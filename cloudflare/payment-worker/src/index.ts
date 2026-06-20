// Subscriptions + real payments (provider-agnostic; lava.top first).
//
// The per-user SubscriptionDO is the single source of truth that every gate reads
// (ai-worker / ocr-queue: GET /subscription → {active}). Payment providers only
// drive its state via webhooks. A lazy 14-day trial is created on first read.
// PaymentIndexDO maps a provider's orderId / contractId back to our user id, since
// webhooks carry the provider's ids (and the buyer email), not our user id.

import { getProvider, type PaymentProvider, type ProviderEnv } from "./providers";

// A Cloudflare Secrets Store binding (read via the async get()).
interface SecretBinding {
  get(): Promise<string>;
}

interface Env {
  SUBSCRIPTION_DO: DurableObjectNamespace;
  PAYMENT_INDEX_DO: DurableObjectNamespace;
  JWT_SECRET: string;
  PLANS?: string; // JSON array of plans (see Plan)
  RETURN_URL?: string; // where the hosted checkout returns the buyer
  // Provider credentials come from the account Secrets Store (binding.get()),
  // but a plain string (wrangler secret / var) is also accepted.
  LAVA_API_KEY?: SecretBinding | string;
  LAVA_WEBHOOK_SECRET?: SecretBinding | string;
}

/** Resolve a Secrets Store binding (or a plain string secret/var) to its value. */
async function readSecret(b: SecretBinding | string | undefined): Promise<string | undefined> {
  if (b == null) return undefined;
  if (typeof b === "string") return b;
  if (typeof b.get === "function") return await b.get();
  return undefined;
}

/** Build a provider with credentials resolved from the Secrets Store. */
async function providerFor(name: string, env: Env): Promise<PaymentProvider | null> {
  const provEnv: ProviderEnv = {
    LAVA_API_KEY:
      (await readSecret(env.LAVA_API_KEY)) ??
      (await readSecret((env as unknown as Record<string, SecretBinding | string>)["lava-top"])),
    LAVA_WEBHOOK_SECRET: await readSecret(env.LAVA_WEBHOOK_SECRET),
  };
  return getProvider(name, provEnv);
}

const DAY_MS = 24 * 60 * 60 * 1000;
const TRIAL_DAYS = 14;
const DEFAULT_PERIOD_DAYS = 30; // fallback when a webhook gives no explicit period end

const CORS_HEADERS: Record<string, string> = {
  "Access-Control-Allow-Methods": "GET, POST, OPTIONS",
  "Access-Control-Allow-Headers": "Content-Type, Authorization",
};

// Known origins only (no wildcard): the prod app + any renorma.app subdomain,
// the dev test env, and localhost for development.
const ALLOWED_ORIGIN_RE =
  /^https:\/\/([a-z0-9-]+\.)*renorma\.app$|^https:\/\/hjkl-ft\.pages\.dev$|^http:\/\/(localhost|127\.0\.0\.1)(:\d+)?$/;

function applyCors(res: Response, request: Request): Response {
  const origin = request.headers.get("Origin");
  const out = new Response(res.body, res);
  out.headers.append("Vary", "Origin");
  if (origin && ALLOWED_ORIGIN_RE.test(origin)) {
    out.headers.set("Access-Control-Allow-Origin", origin);
  }
  return out;
}

// ── Plan catalog ──────────────────────────────────────────────────────────────
interface Plan {
  id: string;
  title: string;
  price: number;
  currency: string;
  period: string; // "month" | "year" | ...
  lavaOfferId?: string; // provider offer id (server-side only)
}
function plans(env: Env): Plan[] {
  if (!env.PLANS) return [];
  try {
    return JSON.parse(env.PLANS) as Plan[];
  } catch {
    return [];
  }
}
function publicPlan(p: Plan) {
  const { lavaOfferId: _omit, ...rest } = p;
  return rest;
}

// ── SubscriptionDO (per user) ─────────────────────────────────────────────────
type SubStatus = "trial" | "paid" | "cancelled" | "expired";
interface SubRecord {
  plan: string; // planId, or "trial"
  status: SubStatus;
  start: number;
  end: number;
  provider?: string;
  contractId?: string;
  lastOrderId?: string;
  no_renew?: boolean;
  order_id?: string;
  transaction_id?: string;
}

function statusOf(rec: SubRecord) {
  return {
    plan: rec.plan,
    status: rec.status,
    start: rec.start,
    end: rec.end,
    active: Date.now() < rec.end,
    provider: rec.provider ?? null,
    contractId: rec.contractId ?? null,
    no_renew: rec.no_renew ?? false,
    order_id: rec.order_id ?? null,
    transaction_id: rec.transaction_id ?? null,
  };
}

export class SubscriptionDO {
  private storage: DurableObjectStorage;
  constructor(state: DurableObjectState) {
    this.storage = state.storage;
  }

  private async load(): Promise<SubRecord> {
    let rec = await this.storage.get<SubRecord>("sub");
    if (!rec) {
      const now = Date.now();
      rec = { plan: "trial", status: "trial", start: now, end: now + TRIAL_DAYS * DAY_MS };
      await this.storage.put("sub", rec);
    }
    return rec;
  }

  async fetch(request: Request): Promise<Response> {
    const url = new URL(request.url);
    const rec = await this.load();

    if (request.method === "GET" && url.pathname === "/subscription") {
      return Response.json(statusOf(rec));
    }

    // Provider-driven: a payment succeeded → mark paid and extend.
    if (request.method === "POST" && url.pathname === "/activate") {
      const b = (await request.json()) as {
        periodEnd?: number; provider?: string; contractId?: string; orderId?: string; planId?: string;
      };
      const now = Date.now();
      rec.status = "paid";
      rec.plan = b.planId ?? rec.plan;
      rec.start = now;
      rec.end = b.periodEnd && b.periodEnd > now ? b.periodEnd : now + DEFAULT_PERIOD_DAYS * DAY_MS;
      rec.provider = b.provider ?? rec.provider;
      rec.contractId = b.contractId ?? rec.contractId;
      rec.lastOrderId = b.orderId ?? rec.lastOrderId;
      rec.no_renew = false;
      rec.order_id = b.orderId ?? rec.order_id;
      await this.storage.put("sub", rec);
      return Response.json(statusOf(rec));
    }

    // Cancel auto-renew: stay active until `end`, then lapse.
    if (request.method === "POST" && url.pathname === "/cancel") {
      rec.no_renew = true;
      rec.status = "cancelled";
      await this.storage.put("sub", rec);
      return Response.json(statusOf(rec));
    }

    // Refund: revoke access immediately.
    if (request.method === "POST" && url.pathname === "/refund") {
      rec.end = Date.now();
      rec.status = "expired";
      await this.storage.put("sub", rec);
      return Response.json(statusOf(rec));
    }

    return new Response("Not found", { status: 404 });
  }
}

// ── PaymentIndexDO (single, global) ───────────────────────────────────────────
// Maps "order:<id>" / "contract:<id>" → userId so webhooks resolve to the user.
export class PaymentIndexDO {
  private storage: DurableObjectStorage;
  constructor(state: DurableObjectState) {
    this.storage = state.storage;
  }
  async fetch(request: Request): Promise<Response> {
    const url = new URL(request.url);
    if (request.method === "POST" && url.pathname === "/put") {
      const { key, userId } = (await request.json()) as { key: string; userId: string };
      await this.storage.put(key, userId);
      return Response.json({ ok: true });
    }
    if (request.method === "GET" && url.pathname === "/get") {
      const key = url.searchParams.get("key") ?? "";
      const userId = (await this.storage.get<string>(key)) ?? null;
      return Response.json({ userId });
    }
    return new Response("Not found", { status: 404 });
  }
}

// ── JWT ───────────────────────────────────────────────────────────────────────
async function verifyJwt(token: string, secret: string): Promise<boolean> {
  const parts = token.split(".");
  if (parts.length !== 3) return false;
  const key = await crypto.subtle.importKey(
    "raw",
    new TextEncoder().encode(secret),
    { name: "HMAC", hash: "SHA-256" },
    false,
    ["verify"],
  );
  const sigBuf = base64UrlDecode(parts[2]);
  const data = new TextEncoder().encode(`${parts[0]}.${parts[1]}`);
  return crypto.subtle.verify("HMAC", key, sigBuf, data);
}
function base64UrlDecode(s: string): ArrayBuffer {
  const padded = s.replace(/-/g, "+").replace(/_/g, "/");
  const binary = atob(padded);
  const bytes = new Uint8Array(binary.length);
  for (let i = 0; i < binary.length; i++) bytes[i] = binary.charCodeAt(i);
  return bytes.buffer;
}
function decodeJwtSub(token: string): string | null {
  const parts = token.split(".");
  if (parts.length !== 3) return null;
  try {
    const json = new TextDecoder().decode(base64UrlDecode(parts[1]));
    const claims = JSON.parse(json) as { sub?: string };
    return typeof claims.sub === "string" ? claims.sub : null;
  } catch {
    return null;
  }
}

function corsJson(body: string, status: number): Response {
  return new Response(body, {
    status,
    headers: { "Content-Type": "application/json", ...CORS_HEADERS },
  });
}
function errorResponse(message: string, status: number): Response {
  return corsJson(JSON.stringify({ error: message }), status);
}

// ── Helpers to talk to the DOs from the worker ────────────────────────────────
function subStub(env: Env, userId: string) {
  return env.SUBSCRIPTION_DO.get(env.SUBSCRIPTION_DO.idFromName(userId));
}
function indexStub(env: Env) {
  return env.PAYMENT_INDEX_DO.get(env.PAYMENT_INDEX_DO.idFromName("index"));
}
async function indexPut(env: Env, key: string, userId: string) {
  await indexStub(env).fetch("https://do/put", {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ key, userId }),
  });
}
async function indexGet(env: Env, key: string): Promise<string | null> {
  const res = await indexStub(env).fetch(`https://do/get?key=${encodeURIComponent(key)}`);
  const { userId } = (await res.json()) as { userId: string | null };
  return userId;
}

export default {
  async fetch(request: Request, env: Env): Promise<Response> {
    return applyCors(await handle(request, env), request);
  },
} satisfies ExportedHandler<Env>;

async function handle(request: Request, env: Env): Promise<Response> {
  if (request.method === "OPTIONS") {
    return new Response(null, { status: 204, headers: CORS_HEADERS });
  }

  const url = new URL(request.url);

  // ── Provider webhooks (NO app JWT — verified by the provider's signature) ──
  if (request.method === "POST" && url.pathname.startsWith("/webhook/")) {
    const name = url.pathname.slice("/webhook/".length);
    const provider = await providerFor(name, env);
    if (!provider) return errorResponse("unknown_provider", 404);

    const v = await provider.verifyWebhook(request);
    if (!v.ok) return errorResponse("invalid_signature", 401);
    const ev = provider.parseWebhook(v.body);

    // Resolve our user id from the provider's order / contract id.
    let userId: string | null = null;
    if (ev.orderId) userId = await indexGet(env, `order:${ev.orderId}`);
    if (!userId && ev.contractId) userId = await indexGet(env, `contract:${ev.contractId}`);
    if (!userId) {
      // Can't map (e.g. a stray event) — ack so the provider stops retrying.
      return corsJson(JSON.stringify({ ok: true, mapped: false }), 200);
    }

    if (ev.contractId) await indexPut(env, `contract:${ev.contractId}`, userId);
    const sub = subStub(env, userId);
    if (ev.kind === "paid" || ev.kind === "recurring") {
      await sub.fetch("https://do/activate", {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({
          periodEnd: ev.periodEnd,
          provider: name,
          contractId: ev.contractId,
          orderId: ev.orderId,
          planId: ev.planId,
        }),
      });
    } else if (ev.kind === "cancelled") {
      await sub.fetch("https://do/cancel", { method: "POST" });
    } else if (ev.kind === "refunded") {
      await sub.fetch("https://do/refund", { method: "POST" });
    }
    return corsJson(JSON.stringify({ ok: true }), 200);
  }

  // ── Everything else is app-JWT authed ──
  const authHeader = request.headers.get("Authorization") ?? "";
  const token = authHeader.startsWith("Bearer ") ? authHeader.slice(7) : "";
  if (!token || !(await verifyJwt(token, env.JWT_SECRET))) return errorResponse("Unauthorized", 401);
  const userId = decodeJwtSub(token);
  if (!userId) return errorResponse("Unauthorized", 401);

  if (request.method === "GET" && url.pathname === "/subscription") {
    const res = await subStub(env, userId).fetch("https://do/subscription");
    return corsJson(await res.text(), res.status);
  }

  if (request.method === "GET" && url.pathname === "/plans") {
    return corsJson(JSON.stringify({ plans: plans(env).map(publicPlan) }), 200);
  }

  if (request.method === "POST" && url.pathname === "/checkout") {
    const body = (await request.json()) as { provider?: string; planId?: string };
    const providerName = body.provider ?? "lava";
    const provider = await providerFor(providerName, env);
    if (!provider || !provider.configured()) return errorResponse("provider_not_configured", 400);
    const plan = plans(env).find((p) => p.id === body.planId);
    if (!plan || !plan.lavaOfferId) return errorResponse("unknown_plan", 400);

    const returnUrl = env.RETURN_URL ?? "https://fit.renorma.app/paywall?status=success";
    try {
      const { url: payUrl, orderId } = await provider.createCheckout({
        userId,
        planId: plan.id,
        offerId: plan.lavaOfferId,
        returnUrl,
      });
      await indexPut(env, `order:${orderId}`, userId);
      return corsJson(JSON.stringify({ url: payUrl }), 200);
    } catch (e) {
      return errorResponse(`checkout_failed: ${(e as Error).message}`, 502);
    }
  }

  if (request.method === "POST" && url.pathname === "/cancel") {
    const res = await subStub(env, userId).fetch("https://do/subscription");
    const cur = (await res.json()) as { provider?: string | null; contractId?: string };
    const providerName = cur.provider ?? "";
    const provider = providerName ? await providerFor(providerName, env) : null;
    if (provider?.cancel && cur.contractId) {
      try {
        await provider.cancel(cur.contractId);
      } catch {
        /* best-effort: still mark no-renew locally */
      }
    }
    const out = await subStub(env, userId).fetch("https://do/cancel", { method: "POST" });
    return corsJson(await out.text(), out.status);
  }

  return errorResponse("Not found", 404);
}

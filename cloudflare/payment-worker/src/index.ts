// Fake paywall + subscription state.
//
// Per-user subscription lives in a `SubscriptionDO` (one instance per user id,
// `idFromName(sub)`). A Trial is created lazily on first read; entering the code
// word flips the plan to Paid and records a synthetic order/transaction id —
// simulating an acquirer callback. The `ai-worker` binds to this same DO
// cross-script to gate AI requests.

interface Env {
  SUBSCRIPTION_DO: DurableObjectNamespace;
  JWT_SECRET: string;
}

const DAY_MS = 24 * 60 * 60 * 1000;
const TRIAL_DAYS = 14;
const PAID_DAYS = 30;
const CODE_WORD = "Жиробасина";

const CORS_HEADERS: Record<string, string> = {
  "Access-Control-Allow-Origin": "*",
  "Access-Control-Allow-Methods": "GET, POST, OPTIONS",
  "Access-Control-Allow-Headers": "Content-Type, Authorization",
};

interface SubRecord {
  plan: "trial" | "paid";
  start: number;
  end: number;
  order_id?: string;
  transaction_id?: string;
}

function statusOf(rec: SubRecord) {
  return {
    plan: rec.plan,
    start: rec.start,
    end: rec.end,
    active: Date.now() < rec.end,
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
      rec = { plan: "trial", start: now, end: now + TRIAL_DAYS * DAY_MS };
      await this.storage.put("sub", rec);
    }
    return rec;
  }

  async fetch(request: Request): Promise<Response> {
    const url = new URL(request.url);

    if (request.method === "GET" && url.pathname === "/subscription") {
      const rec = await this.load();
      return Response.json(statusOf(rec));
    }

    if (request.method === "POST" && url.pathname === "/pay") {
      const body = (await request.json()) as { code_word?: string };
      if (body.code_word !== CODE_WORD) {
        return Response.json({ error: "invalid_code" }, { status: 400 });
      }
      const now = Date.now();
      const rec: SubRecord = {
        plan: "paid",
        start: now,
        end: now + PAID_DAYS * DAY_MS,
        order_id: "order_" + crypto.randomUUID(),
        transaction_id: "txn_" + crypto.randomUUID(),
      };
      await this.storage.put("sub", rec);
      return Response.json(statusOf(rec));
    }

    return new Response("Not found", { status: 404 });
  }
}

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

export default {
  async fetch(request: Request, env: Env): Promise<Response> {
    if (request.method === "OPTIONS") {
      return new Response(null, { status: 204, headers: CORS_HEADERS });
    }

    const authHeader = request.headers.get("Authorization") ?? "";
    const token = authHeader.startsWith("Bearer ") ? authHeader.slice(7) : "";
    if (!token || !(await verifyJwt(token, env.JWT_SECRET))) {
      return errorResponse("Unauthorized", 401);
    }
    const userId = decodeJwtSub(token);
    if (!userId) {
      return errorResponse("Unauthorized", 401);
    }

    const url = new URL(request.url);
    const stub = env.SUBSCRIPTION_DO.get(env.SUBSCRIPTION_DO.idFromName(userId));

    if (request.method === "GET" && url.pathname === "/subscription") {
      const res = await stub.fetch("https://do/subscription");
      return corsJson(await res.text(), res.status);
    }

    if (request.method === "POST" && url.pathname === "/pay") {
      const res = await stub.fetch("https://do/pay", {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: await request.text(),
      });
      return corsJson(await res.text(), res.status);
    }

    return errorResponse("Not found", 404);
  },
} satisfies ExportedHandler<Env>;

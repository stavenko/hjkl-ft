// Cross-device data sync.
//
// Per-user data lives in a `SyncDO` (one instance per user id, `idFromName(sub)`)
// so every device authenticating with the same account hits the same dataset.
// Records merge last-writer-wins by their RFC3339 `updated_at` — mirroring the
// reference SQL in `backend/src/use_cases/sync.rs`. Diary deletions are soft:
// the client pushes a tombstone (`deleted: true`); the DO keeps the tombstone so
// the deletion isn't resurrected by an older push, and `dump` omits it.
//
// Wire format is JSON (the only api::post user — sync — was switched off postcard
// for this worker). Auth is HS256 JWT, `sub` = user id, shared `JWT_SECRET`.

interface Env {
  SYNC_DO: DurableObjectNamespace;
  // In dev this is the plain [vars] string; in prod it is a Secrets Store
  // binding (SecretsStoreSecret with async .get()). Resolve via readSecret().
  JWT_SECRET: string | SecretsStoreSecret;
}

/// Resolve a value that is either a plain [vars] string (dev) or a Secrets Store
/// binding (prod, SecretsStoreSecret with async .get()). Never swallows: a
/// Store-bound secret present but unresolvable (rejects/empty) throws a clear
/// MISCONFIGURED error; an undefined value (configured nowhere) also throws.
async function readSecret(value: string | SecretsStoreSecret | undefined, name: string): Promise<string> {
  if (value === undefined) throw new Error(`MISCONFIGURED: ${name} not set (no Secrets Store binding and no var)`);
  if (typeof value === "string") return value;                 // dev [vars]
  let v: string;
  try { v = await value.get(); }
  catch (e) { throw new Error(`MISCONFIGURED: Secrets Store binding '${name}' get() failed: ${e}`); }
  if (!v) throw new Error(`MISCONFIGURED: Secrets Store binding '${name}' is empty/unset`);
  return v;
}

/// Resolve every REQUIRED Store-bound secret. Returns a 503 Response (and logs
/// the full reason) on the first failure so any request to a misconfigured
/// worker fails loudly instead of degrading to a confusing 401.
async function requireSecrets(env: Env): Promise<Response | null> {
  for (const name of ["JWT_SECRET"]) {
    try { await readSecret((env as any)[name], name); }
    catch (e) {
      console.error(`STARTUP MISCONFIG: ${name}: ${e}`);
      return new Response(`MISCONFIGURED: ${name} — ${e instanceof Error ? e.message : String(e)}`, { status: 503 });
    }
  }
  return null;
}

const CORS_HEADERS: Record<string, string> = {
  "Access-Control-Allow-Methods": "GET, POST, OPTIONS",
  "Access-Control-Allow-Headers": "Content-Type, Authorization",
};

// Known origins only (no wildcard): the prod app + any renorma.app subdomain,
// the dev test env, and localhost for development. The exported fetch is wrapped
// in applyCors, which echoes a matching request Origin into Access-Control-Allow-Origin.
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


// Collections keyed by `id`, last-writer-wins by `updated_at`.
const ID_COLLECTIONS = [
  "foods",
  "diary_entries",
  "recipes",
  "recipe_ingredients",
  "goals",
  "weight_entries",
  "step_entries",
  // Explicit deletion records (tombstones). Append-only by id; the worker never
  // hard-deletes entities — it just accumulates these and re-serves them so every
  // device removes the targets locally.
  "deletions",
] as const;

interface DumpShape {
  foods: any[];
  diary_entries: any[];
  recipes: any[];
  recipe_ingredients: any[];
  goals: any[];
  story: any[];
  profile: any[];
  weight_entries: any[];
  step_entries: any[];
  deletions: any[];
}

/** `true` when `incoming` should overwrite `current` (newer, or current absent). */
function isNewer(incoming: any, current: any | undefined): boolean {
  if (!current) return true;
  return String(incoming.updated_at ?? "") > String(current.updated_at ?? "");
}

export class SyncDO {
  private storage: DurableObjectStorage;

  constructor(state: DurableObjectState) {
    this.storage = state.storage;
  }

  private async map(name: string): Promise<Record<string, any>> {
    return (await this.storage.get<Record<string, any>>(name)) ?? {};
  }

  async fetch(request: Request): Promise<Response> {
    const url = new URL(request.url);

    if (request.method === "POST" && url.pathname === "/sync/dump") {
      return Response.json(await this.dump());
    }

    if (request.method === "POST" && url.pathname === "/sync/push") {
      const payload = (await request.json()) as Partial<DumpShape>;
      await this.push(payload);
      return Response.json({ conflicts: null });
    }

    return new Response("Not found", { status: 404 });
  }

  private async dump(): Promise<DumpShape> {
    const out: any = { story: [] };
    for (const name of ID_COLLECTIONS) {
      const m = await this.map(name);
      out[name] =
        name === "diary_entries"
          ? Object.values(m).filter((r: any) => !r.deleted)
          : Object.values(m);
    }
    const story = await this.map("story");
    out.story = Object.values(story);
    const profile = await this.map("profile");
    out.profile = Object.values(profile);
    return out as DumpShape;
  }

  private async push(payload: Partial<DumpShape>): Promise<void> {
    for (const name of ID_COLLECTIONS) {
      const incoming = payload[name];
      if (!Array.isArray(incoming) || incoming.length === 0) continue;
      const m = await this.map(name);
      for (const row of incoming) {
        if (!row || typeof row.id !== "string") continue;
        const cur = m[row.id];
        if (name === "diary_entries" && row.deleted) {
          // Tombstone: keep it (so dump omits the row) when it's the newer write.
          if (isNewer(row, cur)) m[row.id] = row;
        } else if (isNewer(row, cur)) {
          m[row.id] = row;
        }
      }
      await this.storage.put(name, m);
    }

    if (Array.isArray(payload.story) && payload.story.length > 0) {
      const m = await this.map("story");
      for (const row of payload.story) {
        if (!row || typeof row.key !== "string") continue;
        if (isNewer(row, m[row.key])) m[row.key] = row;
      }
      await this.storage.put("story", m);
    }

    // Profile is a keyed singleton (one key, "profile") — same LWW machinery as
    // story: whole-object last-writer-wins, no per-field merge.
    if (Array.isArray(payload.profile) && payload.profile.length > 0) {
      const m = await this.map("profile");
      for (const row of payload.profile) {
        if (!row || typeof row.key !== "string") continue;
        if (isNewer(row, m[row.key])) m[row.key] = row;
      }
      await this.storage.put("profile", m);
    }
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

const inner = {
  async fetch(request: Request, env: Env): Promise<Response> {
    if (request.method === "OPTIONS") {
      return new Response(null, { status: 204, headers: CORS_HEADERS });
    }

    const misconfig = await requireSecrets(env);
    if (misconfig) return misconfig;

    const authHeader = request.headers.get("Authorization") ?? "";
    const token = authHeader.startsWith("Bearer ") ? authHeader.slice(7) : "";
    if (!token || !(await verifyJwt(token, await readSecret(env.JWT_SECRET, "JWT_SECRET")))) {
      return errorResponse("Unauthorized", 401);
    }
    const userId = decodeJwtSub(token);
    if (!userId) {
      return errorResponse("Unauthorized", 401);
    }

    const url = new URL(request.url);
    if (request.method !== "POST" || (url.pathname !== "/sync/dump" && url.pathname !== "/sync/push")) {
      return errorResponse("Not found", 404);
    }

    const stub = env.SYNC_DO.get(env.SYNC_DO.idFromName(userId));
    const res = await stub.fetch(`https://do${url.pathname}`, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: await request.text(),
    });
    return corsJson(await res.text(), res.status);
  },
} satisfies ExportedHandler<Env>;

export default {
  async fetch(request: Request, env: Env): Promise<Response> {
    return applyCors(await inner.fetch!(request, env), request);
  },
} satisfies ExportedHandler<Env>;

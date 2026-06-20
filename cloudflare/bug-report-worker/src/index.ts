// Bug-report intake.
//
// Authenticated app users file bug reports through the in-app support chat: the
// assistant gathers the details and calls its `file_bug_report` tool, which
// POSTs here. Every report is stored append-only in a single global
// `BugReportDO` (one instance, `idFromName("global")`), stamped with the
// reporting user's id (JWT `sub`) and a server received-at time. JWT-gated, so
// only signed-in app users can reach it.

interface Env {
  BUG_REPORT_DO: DurableObjectNamespace;
  JWT_SECRET: string;
  ADMIN_KEY: string;
}

const CORS_HEADERS: Record<string, string> = {
  "Access-Control-Allow-Methods": "GET, POST, OPTIONS",
  "Access-Control-Allow-Headers": "Content-Type, Authorization, X-Admin-Key",
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


// The report fields the client (assistant tool) sends. The worker adds `user`
// from the JWT and the DO adds `id` + `received_at`.
interface ReportInput {
  title: string;
  area: string;
  steps_to_reproduce: string;
  expected: string;
  actual: string;
  severity: string;
  app_version: string;
}

interface StoredReport extends ReportInput {
  id: string;
  user: string;
  received_at: number;
}

export class BugReportDO {
  private storage: DurableObjectStorage;

  constructor(state: DurableObjectState) {
    this.storage = state.storage;
  }

  async fetch(request: Request): Promise<Response> {
    const url = new URL(request.url);

    if (request.method === "POST" && url.pathname === "/report") {
      const body = (await request.json()) as ReportInput & { user: string };
      const id = "bug_" + crypto.randomUUID();
      const rec: StoredReport = {
        id,
        user: body.user,
        received_at: Date.now(),
        title: body.title ?? "",
        area: body.area ?? "other",
        steps_to_reproduce: body.steps_to_reproduce ?? "",
        expected: body.expected ?? "",
        actual: body.actual ?? "",
        severity: body.severity ?? "medium",
        app_version: body.app_version ?? "",
      };
      // Time-sortable key so the admin read lists newest reports first.
      await this.storage.put(`report:${rec.received_at}:${id}`, rec);
      return Response.json({ id });
    }

    if (request.method === "GET" && url.pathname === "/reports") {
      // `reverse: true` walks keys descending; keys are `report:<ts>:<id>`, so
      // newest reports come first.
      const map = await this.storage.list<StoredReport>({
        prefix: "report:",
        reverse: true,
        limit: 500,
      });
      return Response.json({ reports: [...map.values()] });
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

const inner = {
  async fetch(request: Request, env: Env): Promise<Response> {
    if (request.method === "OPTIONS") {
      return new Response(null, { status: 204, headers: CORS_HEADERS });
    }

    const url = new URL(request.url);
    const stub = env.BUG_REPORT_DO.get(env.BUG_REPORT_DO.idFromName("global"));

    // Admin read: gather the collected reports. Gated by ADMIN_KEY (a developer
    // tool), NOT a user JWT — so one signed-in user can't read others' reports.
    if (request.method === "GET" && url.pathname === "/reports") {
      const adminKey = request.headers.get("X-Admin-Key") ?? "";
      if (!env.ADMIN_KEY || adminKey !== env.ADMIN_KEY) {
        return errorResponse("Unauthorized", 401);
      }
      const res = await stub.fetch("https://do/reports");
      return corsJson(await res.text(), res.status);
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

    if (request.method === "POST" && url.pathname === "/report") {
      const body = (await request.json()) as Record<string, unknown>;
      const res = await stub.fetch("https://do/report", {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ ...body, user: userId }),
      });
      return corsJson(await res.text(), res.status);
    }

    return errorResponse("Not found", 404);
  },
} satisfies ExportedHandler<Env>;

export default {
  async fetch(request: Request, env: Env): Promise<Response> {
    return applyCors(await inner.fetch!(request, env), request);
  },
} satisfies ExportedHandler<Env>;

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
}

const CORS_HEADERS: Record<string, string> = {
  "Access-Control-Allow-Origin": "*",
  "Access-Control-Allow-Methods": "POST, OPTIONS",
  "Access-Control-Allow-Headers": "Content-Type, Authorization",
};

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
      // Time-sortable key so a future admin read lists newest reports easily.
      await this.storage.put(`report:${rec.received_at}:${id}`, rec);
      return Response.json({ id });
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
    const stub = env.BUG_REPORT_DO.get(env.BUG_REPORT_DO.idFromName("global"));

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

interface Env {
  AI: Ai;
  // In dev this is the plain [vars] string; in prod it is a Secrets Store
  // binding (SecretsStoreSecret with async .get()). Resolve via readSecret().
  JWT_SECRET: string | SecretsStoreSecret;
  SUBSCRIPTION_DO: DurableObjectNamespace;
}

/// Resolve a value that is either a plain [vars] string (dev) or a Secrets Store
/// binding (prod, SecretsStoreSecret with async .get()). Never swallows:
/// distinguishes undefined (configured nowhere → throw), a string (dev [vars]
/// → return), and a SecretsStoreSecret (prod → await .get(), throw a clear
/// MISCONFIGURED error if it rejects or resolves empty). Fails loudly so a
/// misconfigured Store binding cannot silently degrade into a confusing 401.
async function readSecret(value: string | SecretsStoreSecret | undefined, name: string): Promise<string> {
  if (value === undefined) throw new Error(`MISCONFIGURED: ${name} not set (no Secrets Store binding and no var)`);
  if (typeof value === "string") return value; // dev [vars]
  let v: string;
  try { v = await value.get(); }
  catch (e) { throw new Error(`MISCONFIGURED: Secrets Store binding '${name}' get() failed: ${e}`); }
  if (!v) throw new Error(`MISCONFIGURED: Secrets Store binding '${name}' is empty/unset`);
  return v;
}

/// Resolve every required Store-bound secret at the top of fetch. Workers have
/// no separate startup, so this runs per-request: any request to a misconfigured
/// worker returns 503 + logs the reason instead of a confusing 401.
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

/// True if the user's paid subscription is still active. Reads the per-user
/// SubscriptionDO owned by payment-worker. There is no trial: a never-paid
/// account reports active:false until it claims a paid guest subscription.
async function subscriptionActive(env: Env, userId: string): Promise<boolean> {
  const stub = env.SUBSCRIPTION_DO.get(env.SUBSCRIPTION_DO.idFromName(userId));
  const res = await stub.fetch("https://do/subscription");
  if (!res.ok) return false;
  const status = (await res.json()) as { active?: boolean };
  return status.active === true;
}

function jsonResponse(body: unknown, status = 200): Response {
  return new Response(JSON.stringify(body), {
    status,
    headers: { "Content-Type": "application/json", ...CORS_HEADERS },
  });
}

function errorResponse(message: string, status: number): Response {
  return jsonResponse({ error: message }, status);
}

// ---- Chat completions: a thin, model-parametrized proxy over Workers AI ----
//
// This is the single AI entrypoint. Callers pass the `model` and the messages
// (multimodal `image_url` parts included), so both nutrition text lookups and
// label-vision requests go through here. Nutrition post-processing (unit
// conversion, custom-nutrient enrichment) lives in the client.

type ContentPart =
  | { type: "text"; text: string }
  | { type: "image_url"; image_url: { url: string } };

interface ChatMessage {
  role: string;
  content: string | ContentPart[];
}

interface ChatCompletionRequest {
  model: string;
  messages: ChatMessage[];
  response_format?: {
    type: string;
    json_schema?: { name: string; schema: Record<string, unknown>; strict: boolean };
  };
  stream?: boolean;
  chat_template_kwargs?: { enable_thinking?: boolean };
  max_tokens?: number;
}

function resolveRefs(node: unknown, defs: Record<string, unknown>): unknown {
  if (node === null || typeof node !== "object") return node;
  if (Array.isArray(node)) return node.map((item) => resolveRefs(item, defs));

  const obj = node as Record<string, unknown>;
  if (typeof obj["$ref"] === "string") {
    const refPath = obj["$ref"] as string;
    const defName = refPath.replace("#/$defs/", "").replace("#/definitions/", "");
    const resolved = defs[defName];
    if (resolved) return resolveRefs(resolved, defs);
    return obj;
  }

  const result: Record<string, unknown> = {};
  for (const [key, value] of Object.entries(obj)) {
    if (key === "$defs" || key === "definitions" || key === "$schema" || key === "title") continue;
    result[key] = resolveRefs(value, defs);
  }
  return result;
}

function inlineSchema(schema: Record<string, unknown>): Record<string, unknown> {
  const defs = (schema["$defs"] ?? schema["definitions"] ?? {}) as Record<string, unknown>;
  return resolveRefs(schema, defs) as Record<string, unknown>;
}

function hasImageContent(messages: ChatMessage[]): boolean {
  return messages.some(
    (m) => Array.isArray(m.content) && m.content.some((p) => p?.type === "image_url"),
  );
}

async function handleChatCompletions(request: Request, env: Env): Promise<Response> {
  const body = (await request.json()) as ChatCompletionRequest;

  const messages = [...body.messages];
  if (body.response_format?.json_schema?.schema) {
    const schema = inlineSchema(body.response_format.json_schema.schema);
    const schemaJson = JSON.stringify(schema);
    const jsonInstruction =
      `\n\nYou MUST respond with ONLY valid JSON (no markdown, no explanation, no code fences). ` +
      `The JSON MUST conform to this exact schema:\n${schemaJson}`;
    const sysIdx = messages.findIndex((m) => m.role === "system");
    if (sysIdx >= 0 && typeof messages[sysIdx].content === "string") {
      messages[sysIdx] = {
        ...messages[sysIdx],
        content: (messages[sysIdx].content as string) + jsonInstruction,
      };
    } else {
      messages.unshift({ role: "system", content: `You are a helpful assistant.${jsonInstruction}` });
    }
  }

  const images = hasImageContent(messages);
  const wantStream = body.stream ?? true;

  // Reasoning control. Text reasoning models (GLM, qwen3) need thinking ON so
  // they produce data rather than echoing the schema; vision models (llama)
  // have no such chat-template kwarg, so we pass none for image requests. A
  // client may override explicitly via `chat_template_kwargs`.
  const runParams: Record<string, unknown> = { messages, stream: wantStream };
  if (body.chat_template_kwargs) {
    runParams.chat_template_kwargs = body.chat_template_kwargs;
  } else if (!images) {
    runParams.chat_template_kwargs = { enable_thinking: true };
  }
  // Forward the client's max_tokens (the runParams previously dropped it, so it
  // never reached the model — output stayed pinned at the Workers AI default).
  if (typeof body.max_tokens === "number") {
    runParams.max_tokens = body.max_tokens;
  }

  // Plain string selects the generic `run(model, Record)` overload, which
  // accepts both text and vision model ids.
  const model: string = body.model;

  // The worker does NO response parsing/processing. The raw Workers AI output —
  // stream or JSON — is passed straight through; the FRONTEND assembles and
  // parses the fully-received content. (Previously the worker re-parsed each SSE
  // chunk and re-emitted it from a corrupted field, which silently mangled
  // numbers/quotes mid-stream.)
  if (!wantStream) {
    const out = await env.AI.run(model, runParams);
    return jsonResponse(out as unknown as Record<string, unknown>);
  }

  const aiStream = await env.AI.run(model, runParams);
  return new Response(aiStream as unknown as ReadableStream, {
    headers: { "Content-Type": "text/event-stream", "Cache-Control": "no-cache", ...CORS_HEADERS },
  });
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

    const url = new URL(request.url);

    if (request.method !== "POST") {
      return errorResponse("Not found", 404);
    }

    if (url.pathname === "/chat/completions") {
      // Gate AI on an active subscription (Trial not expired, or Paid).
      const userId = decodeJwtSub(token);
      if (!userId || !(await subscriptionActive(env, userId))) {
        return errorResponse("subscription_required", 402);
      }
      return handleChatCompletions(request, env);
    }
    return errorResponse("Not found", 404);
  },
} satisfies ExportedHandler<Env>;

export default {
  async fetch(request: Request, env: Env): Promise<Response> {
    return applyCors(await inner.fetch!(request, env), request);
  },
} satisfies ExportedHandler<Env>;

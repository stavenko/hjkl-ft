// OCR job queue.
//
// The PWA submits a label image; the job, its status AND the image (chunked)
// live in a single Durable Object pinned to Western Europe (QUEUE_REGION). An
// on-prem poller (GPU box, behind a VPN that egresses in Italy) claims jobs,
// runs Qwen2.5-VL locally, and posts the result back. The client polls status.
//
// Auth: client routes use the shared app JWT; poller routes use POLLER_SECRET.
// The image is carried as base64 throughout — that's exactly what the vision
// model wants as a data URL, so no byte juggling is needed.

interface Env {
  QUEUE_DO: DurableObjectNamespace;
  SUBSCRIPTION_DO: DurableObjectNamespace;
  JWT_SECRET: string;
  POLLER_SECRET: string;
  QUEUE_REGION: DurableObjectLocationHint;
}

const CORS_HEADERS: Record<string, string> = {
  "Access-Control-Allow-Origin": "*",
  "Access-Control-Allow-Methods": "GET, POST, OPTIONS",
  "Access-Control-Allow-Headers": "Content-Type, Authorization",
};

// Base64 chunk size kept well under the SQLite-backed DO per-value limit.
const CHUNK = 700_000;

type JobStatus = "queued" | "processing" | "done" | "error";

interface Job {
  id: string;
  status: JobStatus;
  owner: string;
  custom_nutrients: unknown[];
  chunks: number;
  created_at: number;
  started_at?: number;
  updated_at: number;
  // Live LLM phase reported by the poller while processing.
  phase?: "thinking" | "answer";
  thinking_tokens?: number;
  answer_tokens?: number;
  result?: unknown;
  error?: string;
}

export class QueueDO {
  private storage: DurableObjectStorage;

  constructor(state: DurableObjectState) {
    this.storage = state.storage;
  }

  private async queueIds(): Promise<string[]> {
    return (await this.storage.get<string[]>("queue")) ?? [];
  }

  async fetch(request: Request): Promise<Response> {
    const url = new URL(request.url);

    if (url.pathname === "/enqueue" && request.method === "POST") {
      const { id, owner, custom_nutrients, image_b64 } = (await request.json()) as {
        id: string; owner: string; custom_nutrients: unknown[]; image_b64: string;
      };
      let n = 0;
      for (let i = 0; i < image_b64.length; i += CHUNK) {
        await this.storage.put(`img:${id}:${n}`, image_b64.slice(i, i + CHUNK));
        n++;
      }
      const job: Job = {
        id, status: "queued", owner: owner ?? "", custom_nutrients: custom_nutrients ?? [],
        chunks: n, created_at: Date.now(), updated_at: Date.now(),
      };
      await this.storage.put(`job:${id}`, job);
      const q = await this.queueIds();
      q.push(id);
      await this.storage.put("queue", q);
      return Response.json({ ok: true });
    }

    if (url.pathname === "/claim") {
      const q = await this.queueIds();
      while (q.length) {
        const id = q.shift()!;
        const job = await this.storage.get<Job>(`job:${id}`);
        if (!job || job.status !== "queued") continue;
        job.status = "processing";
        job.started_at = Date.now();
        job.updated_at = Date.now();
        await this.storage.put(`job:${id}`, job);
        await this.storage.put("queue", q);
        return Response.json({ job_id: id, custom_nutrients: job.custom_nutrients });
      }
      await this.storage.put("queue", q);
      return Response.json({});
    }

    if (url.pathname === "/image") {
      const id = url.searchParams.get("id")!;
      const job = await this.storage.get<Job>(`job:${id}`);
      if (!job) return new Response("not found", { status: 404 });
      let b64 = "";
      for (let i = 0; i < job.chunks; i++) b64 += (await this.storage.get<string>(`img:${id}:${i}`)) ?? "";
      return new Response(b64, { headers: { "Content-Type": "text/plain" } });
    }

    if (url.pathname === "/progress" && request.method === "POST") {
      const b = (await request.json()) as {
        job_id: string; phase?: "thinking" | "answer"; thinking_tokens?: number; answer_tokens?: number;
      };
      const job = await this.storage.get<Job>(`job:${b.job_id}`);
      if (!job) return Response.json({ error: "unknown job" }, { status: 404 });
      job.phase = b.phase;
      if (typeof b.thinking_tokens === "number") job.thinking_tokens = b.thinking_tokens;
      if (typeof b.answer_tokens === "number") job.answer_tokens = b.answer_tokens;
      job.updated_at = Date.now();
      await this.storage.put(`job:${b.job_id}`, job);
      return Response.json({ ok: true });
    }

    if (url.pathname === "/complete" && request.method === "POST") {
      const body = (await request.json()) as { job_id: string; result?: unknown; error?: string };
      const job = await this.storage.get<Job>(`job:${body.job_id}`);
      if (!job) return Response.json({ error: "unknown job" }, { status: 404 });
      if (body.error) { job.status = "error"; job.error = String(body.error); }
      else { job.status = "done"; job.result = body.result; }
      job.updated_at = Date.now();
      await this.storage.put(`job:${body.job_id}`, job);
      // Free the image chunks once the job is finished.
      for (let i = 0; i < job.chunks; i++) await this.storage.delete(`img:${body.job_id}:${i}`);
      return Response.json({ ok: true });
    }

    if (url.pathname === "/status") {
      const id = url.searchParams.get("id")!;
      const job = await this.storage.get<Job>(`job:${id}`);
      if (!job) return Response.json({ error: "unknown job" }, { status: 404 });
      // 1-based position in the FIFO queue (0 when not waiting).
      let position = 0;
      if (job.status === "queued") {
        const q = await this.queueIds();
        const idx = q.indexOf(id);
        position = idx >= 0 ? idx + 1 : 0;
      }
      return Response.json({
        status: job.status, owner: job.owner, position,
        result: job.result ?? null, error: job.error ?? null,
        created_at: job.created_at, started_at: job.started_at ?? null,
        phase: job.phase ?? null,
        thinking_tokens: job.thinking_tokens ?? 0,
        answer_tokens: job.answer_tokens ?? 0,
      });
    }

    // Long-poll: block until the job changes vs `since` (updated_at) or it
    // finishes, or ~20s pass. Drives the worker's SSE /stream cheaply (one
    // subrequest per change instead of busy-polling).
    if (url.pathname === "/tail") {
      const id = url.searchParams.get("id")!;
      const since = Number(url.searchParams.get("since") ?? "0");
      const deadline = Date.now() + 20000;
      for (;;) {
        const job = await this.storage.get<Job>(`job:${id}`);
        if (!job) return Response.json({ error: "unknown job" }, { status: 404 });
        const terminal = job.status === "done" || job.status === "error";
        if (job.updated_at > since || terminal || Date.now() >= deadline) {
          return Response.json({
            status: job.status,
            phase: job.phase ?? null,
            thinking_tokens: job.thinking_tokens ?? 0,
            answer_tokens: job.answer_tokens ?? 0,
            updated_at: job.updated_at,
            owner: job.owner,
            done: job.status === "done",
            error: job.error ?? null,
            result: job.status === "done" ? (job.result ?? null) : null,
          });
        }
        await new Promise((r) => setTimeout(r, 250));
      }
    }

    return new Response("Not found", { status: 404 });
  }
}

// ---- Worker (HTTP front) ----

function corsJson(body: unknown, status = 200): Response {
  return new Response(JSON.stringify(body), {
    status,
    headers: { "Content-Type": "application/json", ...CORS_HEADERS },
  });
}

async function verifyJwt(token: string, secret: string): Promise<boolean> {
  const parts = token.split(".");
  if (parts.length !== 3) return false;
  const key = await crypto.subtle.importKey(
    "raw", new TextEncoder().encode(secret),
    { name: "HMAC", hash: "SHA-256" }, false, ["verify"],
  );
  const sig = base64UrlDecode(parts[2]);
  const data = new TextEncoder().encode(`${parts[0]}.${parts[1]}`);
  return crypto.subtle.verify("HMAC", key, sig, data);
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

/** Strip an optional data-URL prefix, returning pure base64. */
function toBase64(image: string): string {
  const comma = image.indexOf(",");
  return image.startsWith("data:") && comma >= 0 ? image.slice(comma + 1) : image;
}

function bearer(request: Request): string {
  const h = request.headers.get("Authorization") ?? "";
  return h.startsWith("Bearer ") ? h.slice(7) : "";
}

/// Active subscription (Trial not expired, or Paid) for `userId`. Reads the
/// per-user SubscriptionDO owned by payment-worker (lazily creates a Trial).
async function subscriptionActive(env: Env, userId: string): Promise<boolean> {
  const stub = env.SUBSCRIPTION_DO.get(env.SUBSCRIPTION_DO.idFromName(userId));
  const res = await stub.fetch("https://do/subscription");
  if (!res.ok) return false;
  const status = (await res.json()) as { active?: boolean };
  return status.active === true;
}

export default {
  async fetch(request: Request, env: Env): Promise<Response> {
    if (request.method === "OPTIONS") {
      return new Response(null, { status: 204, headers: CORS_HEADERS });
    }

    const url = new URL(request.url);
    const stub = env.QUEUE_DO.get(env.QUEUE_DO.idFromName("global"), {
      locationHint: env.QUEUE_REGION,
    });

    // ----- Client routes (app JWT) -----
    if (url.pathname === "/submit" && request.method === "POST") {
      const token = bearer(request);
      if (!token || !(await verifyJwt(token, env.JWT_SECRET))) return corsJson({ error: "Unauthorized" }, 401);
      const sub = decodeJwtSub(token);
      if (!sub) return corsJson({ error: "Unauthorized" }, 401);
      // Only enqueue for users with an active subscription (Trial/Paid).
      if (!(await subscriptionActive(env, sub))) return corsJson({ error: "subscription_required" }, 402);

      const body = (await request.json()) as {
        image?: string; images?: string[]; custom_nutrients?: unknown[];
      };
      const images = body.images ?? (body.image ? [body.image] : []);
      if (!images.length) return corsJson({ error: "missing image(s)" }, 400);

      const jobId = crypto.randomUUID();
      // The blob is a JSON array of pure-base64 images (front/back of a label).
      await stub.fetch("https://do/enqueue", {
        method: "POST",
        body: JSON.stringify({
          id: jobId, owner: sub,
          custom_nutrients: body.custom_nutrients ?? [],
          image_b64: JSON.stringify(images.map(toBase64)),
        }),
      });
      return corsJson({ job_id: jobId });
    }

    if (url.pathname.startsWith("/job/") && request.method === "GET") {
      const token = bearer(request);
      if (!token || !(await verifyJwt(token, env.JWT_SECRET))) return corsJson({ error: "Unauthorized" }, 401);
      const sub = decodeJwtSub(token);
      const jobId = url.pathname.slice("/job/".length);
      const res = await stub.fetch(`https://do/status?id=${encodeURIComponent(jobId)}`);
      const data = (await res.json()) as { owner?: string };
      if (res.status === 200 && data.owner && data.owner !== sub) return corsJson({ error: "forbidden" }, 403);
      return corsJson(data, res.status);
    }

    // SSE stream of live LLM progress for a job in `processing`. The worker
    // long-polls the DO (one subrequest per change) and forwards phase/tokens.
    if (url.pathname.startsWith("/stream/") && request.method === "GET") {
      const token = bearer(request);
      if (!token || !(await verifyJwt(token, env.JWT_SECRET))) return corsJson({ error: "Unauthorized" }, 401);
      const sub = decodeJwtSub(token);
      const jobId = url.pathname.slice("/stream/".length);

      const { readable, writable } = new TransformStream();
      const writer = writable.getWriter();
      const enc = new TextEncoder();
      const send = (o: unknown) => writer.write(enc.encode(`data: ${JSON.stringify(o)}\n\n`));
      (async () => {
        let since = 0;
        try {
          for (;;) {
            const res = await stub.fetch(`https://do/tail?id=${encodeURIComponent(jobId)}&since=${since}`);
            if (res.status === 404) { await send({ type: "error", error: "unknown job" }); break; }
            const d = (await res.json()) as {
              owner?: string; updated_at?: number; done?: boolean; error?: string | null;
              phase?: string | null; thinking_tokens?: number; answer_tokens?: number; result?: unknown;
            };
            if (d.owner && sub && d.owner !== sub) { await send({ type: "error", error: "forbidden" }); break; }
            since = d.updated_at ?? since;
            if (d.done) { await send({ type: "done", result: d.result ?? null }); break; }
            if (d.error) { await send({ type: "error", error: d.error }); break; }
            await send({ type: "progress", phase: d.phase ?? null, thinking_tokens: d.thinking_tokens ?? 0, answer_tokens: d.answer_tokens ?? 0 });
          }
        } finally {
          await writer.close();
        }
      })();
      return new Response(readable, {
        headers: { "Content-Type": "text/event-stream", "Cache-Control": "no-cache", ...CORS_HEADERS },
      });
    }

    // ----- Poller routes (POLLER_SECRET) -----
    const isPoller = bearer(request) === env.POLLER_SECRET;

    if (url.pathname === "/claim" && request.method === "POST") {
      if (!isPoller) return corsJson({ error: "Unauthorized" }, 401);
      const res = await stub.fetch("https://do/claim", { method: "POST", body: "{}" });
      return corsJson(await res.json(), res.status);
    }

    if (url.pathname.startsWith("/image/") && request.method === "GET") {
      if (!isPoller) return corsJson({ error: "Unauthorized" }, 401);
      const jobId = url.pathname.slice("/image/".length);
      const res = await stub.fetch(`https://do/image?id=${encodeURIComponent(jobId)}`);
      return new Response(res.body, { status: res.status, headers: { "Content-Type": "text/plain" } });
    }

    if (url.pathname === "/progress" && request.method === "POST") {
      if (!isPoller) return corsJson({ error: "Unauthorized" }, 401);
      const res = await stub.fetch("https://do/progress", { method: "POST", body: await request.text() });
      return corsJson(await res.json(), res.status);
    }

    if (url.pathname === "/complete" && request.method === "POST") {
      if (!isPoller) return corsJson({ error: "Unauthorized" }, 401);
      const body = (await request.json()) as { job_id?: string };
      const res = await stub.fetch("https://do/complete", { method: "POST", body: JSON.stringify(body) });
      return corsJson(await res.json(), res.status);
    }

    return corsJson({ error: "Not found" }, 404);
  },
} satisfies ExportedHandler<Env>;

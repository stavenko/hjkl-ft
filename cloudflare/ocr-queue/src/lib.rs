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

use wasm_bindgen::JsValue;
use worker::*;

mod queue_do;
mod token;

pub use queue_do::QueueDO;

use token::{decode_jwt_sub, secret_or_var, verify_jwt};

// ── CORS ────────────────────────────────────────────────────────────────────
// Known origins only (no wildcard): the prod app + any renorma.app subdomain,
// the dev test env, and localhost for development. Mirrors the TS
// ALLOWED_ORIGIN_RE exactly.
fn is_allowed_origin(origin: &str) -> bool {
    // ^https://([a-z0-9-]+\.)*renorma\.app$
    let renorma = origin
        .strip_prefix("https://")
        .map(|host| {
            host == "renorma.app"
                || host.strip_suffix(".renorma.app").map_or(false, |labels| {
                    !labels.is_empty()
                        && labels.split('.').all(|l| {
                            !l.is_empty()
                                && l.chars()
                                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
                        })
                })
        })
        .unwrap_or(false);
    renorma
        || origin == "https://renorma-fit-dev.pages.dev"
        || origin == "http://localhost"
        || origin.starts_with("http://localhost:")
        || origin == "http://127.0.0.1"
        || origin.starts_with("http://127.0.0.1:")
}

/// Mirror the TS `applyCors`: append `Vary: Origin` and, when the request Origin
/// is allowed, echo it into `Access-Control-Allow-Origin`. Wraps EVERY response
/// (including the 204 preflight and the SSE stream).
fn apply_cors(resp: Response, origin: &str) -> Result<Response> {
    let headers = resp.headers().clone();
    let _ = headers.append("Vary", "Origin");
    if !origin.is_empty() && is_allowed_origin(origin) {
        let _ = headers.set("Access-Control-Allow-Origin", origin);
    }
    let status = resp.status_code();
    Ok(Response::from_body(resp.body().clone())?
        .with_headers(headers)
        .with_status(status))
}

/// The base CORS headers TS attaches via CORS_HEADERS / corsJson (method +
/// allowed-headers). Allow-Origin is added later by `apply_cors`.
fn base_cors_headers() -> Headers {
    let headers = Headers::new();
    let _ = headers.set("Access-Control-Allow-Methods", "GET, POST, OPTIONS");
    let _ = headers.set("Access-Control-Allow-Headers", "Content-Type, Authorization");
    headers
}

/// `corsJson(body, status)` — JSON body with Content-Type + base CORS headers.
fn cors_json(body: &serde_json::Value, status: u16) -> Result<Response> {
    let headers = base_cors_headers();
    let _ = headers.set("Content-Type", "application/json");
    let text = serde_json::to_string(body)
        .map_err(|e| Error::RustError(format!("serialize json: {e}")))?;
    Ok(Response::ok(text)?.with_headers(headers).with_status(status))
}

fn bearer(req: &Request) -> String {
    let h = req
        .headers()
        .get("Authorization")
        .ok()
        .flatten()
        .unwrap_or_default();
    h.strip_prefix("Bearer ").map(String::from).unwrap_or_default()
}

/// Strip an optional data-URL prefix, returning pure base64 (mirrors TS `toBase64`).
fn to_base64(image: &str) -> String {
    if image.starts_with("data:") {
        if let Some(idx) = image.find(',') {
            return image[idx + 1..].to_string();
        }
    }
    image.to_string()
}

// ── DO-stub helpers ───────────────────────────────────────────────────────────

/// The single global QueueDO stub, pinned to QUEUE_REGION (locationHint).
fn queue_stub(env: &Env) -> Result<worker::durable::Stub> {
    let region = env
        .var("QUEUE_REGION")
        .map(|v| v.to_string())
        .unwrap_or_default();
    let ns = env.durable_object("QUEUE_DO")?;
    let id = ns.id_from_name("global")?;
    // locationHint pin (weur). The 0.8 API takes the hint on get_stub_with_location_hint.
    if region.is_empty() {
        id.get_stub()
    } else {
        id.get_stub_with_location_hint(&region)
    }
}

async fn do_get(stub: &worker::durable::Stub, url: &str) -> Result<Response> {
    stub.fetch_with_str(url).await
}

async fn do_post_body(stub: &worker::durable::Stub, url: &str, body: String) -> Result<Response> {
    let mut init = RequestInit::new();
    init.with_method(Method::Post)
        .with_body(Some(JsValue::from_str(&body)));
    let req = Request::new_with_init(url, &init)?;
    stub.fetch_with_request(req).await
}

/// Active subscription (Trial not expired, or Paid) for the caller.
///
/// Delegates to payment-worker — the OWNER of the SubscriptionDO — over the
/// PAYMENT service binding, forwarding the caller's bearer (the same JWT
/// payment-worker validates). payment-worker resolves the DO by its own private
/// epoch, so this worker holds NO knowledge of the DO's name/epoch. That coupling
/// drifted once (this reader stuck on an old epoch → empty DO → active:false →
/// spurious 402 paywall); delegating removes it entirely.
async fn subscription_active(env: &Env, authorization: &str) -> Result<bool> {
    let headers = Headers::new();
    headers.set("Authorization", authorization)?;
    let mut init = RequestInit::new();
    init.with_method(Method::Get).with_headers(headers);
    // Host is irrelevant for a service-binding fetch; only the path routes.
    let req = Request::new_with_init("https://payment-worker/subscription", &init)?;
    let mut res = env.service("PAYMENT")?.fetch_request(req).await?;
    if res.status_code() != 200 {
        return Ok(false);
    }
    let v: serde_json::Value = res.json().await?;
    Ok(v.get("active").and_then(|b| b.as_bool()) == Some(true))
}

/// Relay a DO response body verbatim with the original status, wrapped as JSON
/// with base CORS headers (mirrors `corsJson(await res.json(), res.status)`).
async fn relay_cors(mut res: Response) -> Result<Response> {
    let status = res.status_code();
    let v: serde_json::Value = res.json().await.unwrap_or(serde_json::Value::Null);
    cors_json(&v, status)
}

// ── fail-loud secrets ──────────────────────────────────────────────────────────
/// Resolve every REQUIRED Store-bound secret at the top of fetch. On the first
/// failure: log loudly and return a 503 so ANY request makes the misconfig obvious.
async fn require_secrets(env: &Env) -> std::result::Result<(), Response> {
    for name in ["JWT_SECRET", "POLLER_SECRET"] {
        if let Err(reason) = secret_or_var(env, name).await {
            console_error!("STARTUP MISCONFIG: {name}: {reason}");
            let body = format!("MISCONFIGURED: {name} — {reason}");
            return Err(Response::error(body, 503)
                .unwrap_or_else(|_| Response::error("MISCONFIGURED", 503).unwrap()));
        }
    }
    Ok(())
}

#[event(fetch)]
async fn main(req: Request, env: Env, _ctx: Context) -> Result<Response> {
    let origin = req
        .headers()
        .get("Origin")
        .ok()
        .flatten()
        .unwrap_or_default();

    // OPTIONS / preflight short-circuit (before require_secrets).
    if req.method() == Method::Options {
        let headers = base_cors_headers();
        let resp = Response::empty()?.with_headers(headers).with_status(204);
        return apply_cors(resp, &origin);
    }

    // Unauthenticated liveness probe (frontend `net` service). Wildcard CORS +
    // before secrets so it's a cheap, always-answerable 200 from any origin.
    if req.method() == Method::Get && req.url().map(|u| u.path() == "/health").unwrap_or(false) {
        let headers = Headers::new();
        let _ = headers.set("Access-Control-Allow-Origin", "*");
        let _ = headers.set("Cache-Control", "no-store");
        return Ok(Response::ok("ok")?.with_headers(headers));
    }

    if let Err(resp) = require_secrets(&env).await {
        return apply_cors(resp, &origin);
    }

    let resp = match handle(req, &env).await {
        Ok(r) => r,
        Err(e) => cors_json(&serde_json::json!({ "error": e.to_string() }), 500)?,
    };
    apply_cors(resp, &origin)
}

async fn handle(mut req: Request, env: &Env) -> Result<Response> {
    let url = req.url()?;
    let path = url.path().to_string();
    let method = req.method();
    let stub = queue_stub(env)?;

    // ----- Client routes (app JWT) -----
    if path == "/submit" && method == Method::Post {
        let token = bearer(&req);
        let jwt_secret = secret_or_var(env, "JWT_SECRET").await.map_err(Error::RustError)?;
        if token.is_empty() || !verify_jwt(&token, &jwt_secret) {
            return cors_json(&serde_json::json!({ "error": "Unauthorized" }), 401);
        }
        let sub = match decode_jwt_sub(&token) {
            Some(s) => s,
            None => return cors_json(&serde_json::json!({ "error": "Unauthorized" }), 401),
        };
        // Only enqueue for users with an active subscription (Trial/Paid). The
        // bearer is forwarded to payment-worker, which owns the subscription.
        if !subscription_active(env, &format!("Bearer {token}")).await? {
            return cors_json(&serde_json::json!({ "error": "subscription_required" }), 402);
        }

        let body: serde_json::Value = match req.json().await {
            Ok(v) => v,
            Err(e) => return cors_json(&serde_json::json!({ "error": format!("invalid JSON body: {e}") }), 400),
        };
        // images = body.images ?? (body.image ? [body.image] : [])
        let mut images: Vec<String> = Vec::new();
        if let Some(arr) = body.get("images").and_then(|v| v.as_array()) {
            for it in arr {
                if let Some(s) = it.as_str() {
                    images.push(s.to_string());
                }
            }
        } else if let Some(img) = body.get("image").and_then(|v| v.as_str()) {
            if !img.is_empty() {
                images.push(img.to_string());
            }
        }
        if images.is_empty() {
            return cors_json(&serde_json::json!({ "error": "missing image(s)" }), 400);
        }

        let job_id = uuid_v4();
        let custom_nutrients = body
            .get("custom_nutrients")
            .cloned()
            .unwrap_or(serde_json::json!([]));
        // The blob is a JSON array of pure-base64 images (front/back of a label).
        let stripped: Vec<String> = images.iter().map(|i| to_base64(i)).collect();
        let image_b64 = serde_json::to_string(&stripped)
            .map_err(|e| Error::RustError(format!("serialize images: {e}")))?;
        let enqueue_body = serde_json::json!({
            "id": job_id,
            "owner": sub,
            "custom_nutrients": custom_nutrients,
            "image_b64": image_b64,
        })
        .to_string();
        do_post_body(&stub, "https://do/enqueue", enqueue_body).await?;
        return cors_json(&serde_json::json!({ "job_id": job_id }), 200);
    }

    if path.starts_with("/job/") && method == Method::Get {
        let token = bearer(&req);
        let jwt_secret = secret_or_var(env, "JWT_SECRET").await.map_err(Error::RustError)?;
        if token.is_empty() || !verify_jwt(&token, &jwt_secret) {
            return cors_json(&serde_json::json!({ "error": "Unauthorized" }), 401);
        }
        let sub = decode_jwt_sub(&token);
        let job_id = &path["/job/".len()..];
        let mut res = do_get(
            &stub,
            &format!(
                "https://do/status?id={}",
                js_sys::encode_uri_component(job_id).as_string().unwrap_or_default()
            ),
        )
        .await?;
        let status = res.status_code();
        let data: serde_json::Value = res.json().await.unwrap_or(serde_json::Value::Null);
        let owner = data.get("owner").and_then(|v| v.as_str());
        // TS: res.status===200 && data.owner && data.owner !== sub  (owner truthy =
        // non-empty string; a missing/None sub never equals a non-empty owner).
        if status == 200 {
            if let Some(o) = owner {
                if !o.is_empty() && sub.as_deref() != Some(o) {
                    return cors_json(&serde_json::json!({ "error": "forbidden" }), 403);
                }
            }
        }
        return cors_json(&data, status);
    }

    // SSE stream of live LLM progress for a job in `processing`.
    if path.starts_with("/stream/") && method == Method::Get {
        let token = bearer(&req);
        let jwt_secret = secret_or_var(env, "JWT_SECRET").await.map_err(Error::RustError)?;
        if token.is_empty() || !verify_jwt(&token, &jwt_secret) {
            return cors_json(&serde_json::json!({ "error": "Unauthorized" }), 401);
        }
        let sub = decode_jwt_sub(&token);
        let job_id = path["/stream/".len()..].to_string();
        return stream_response(env, job_id, sub).await;
    }

    // ----- Poller routes (POLLER_SECRET) -----
    let poller_secret = secret_or_var(env, "POLLER_SECRET").await.map_err(Error::RustError)?;
    let is_poller = bearer(&req) == poller_secret;

    if path == "/claim" && method == Method::Post {
        if !is_poller {
            return cors_json(&serde_json::json!({ "error": "Unauthorized" }), 401);
        }
        let res = do_post_body(&stub, "https://do/claim", "{}".to_string()).await?;
        return relay_cors(res).await;
    }

    if path.starts_with("/image/") && method == Method::Get {
        if !is_poller {
            return cors_json(&serde_json::json!({ "error": "Unauthorized" }), 401);
        }
        let job_id = &path["/image/".len()..];
        let mut res = do_get(
            &stub,
            &format!(
                "https://do/image?id={}",
                js_sys::encode_uri_component(job_id).as_string().unwrap_or_default()
            ),
        )
        .await?;
        let status = res.status_code();
        let text = res.text().await?;
        let headers = Headers::new();
        let _ = headers.set("Content-Type", "text/plain");
        return Ok(Response::ok(text)?.with_status(status).with_headers(headers));
    }

    if path == "/progress" && method == Method::Post {
        if !is_poller {
            return cors_json(&serde_json::json!({ "error": "Unauthorized" }), 401);
        }
        let raw = req.text().await.unwrap_or_default();
        let res = do_post_body(&stub, "https://do/progress", raw).await?;
        return relay_cors(res).await;
    }

    if path == "/complete" && method == Method::Post {
        if !is_poller {
            return cors_json(&serde_json::json!({ "error": "Unauthorized" }), 401);
        }
        let body: serde_json::Value = match req.json().await {
            Ok(v) => v,
            Err(e) => return cors_json(&serde_json::json!({ "error": format!("invalid JSON body: {e}") }), 400),
        };
        let res = do_post_body(&stub, "https://do/complete", body.to_string()).await?;
        return relay_cors(res).await;
    }

    cors_json(&serde_json::json!({ "error": "Not found" }), 404)
}

/// SSE stream: the worker long-polls the DO (`/tail`, one subrequest per change)
/// and forwards phase/tokens as `data: {...}\n\n` events. Mirrors the TS
/// TransformStream pump exactly.
async fn stream_response(env: &Env, job_id: String, sub: Option<String>) -> Result<Response> {
    let stub = queue_stub(env)?;

    let s = async_stream::try_stream! {
        let mut since: i64 = 0;
        loop {
            let mut res = stub
                .fetch_with_str(&format!(
                    "https://do/tail?id={}&since={}",
                    js_sys::encode_uri_component(&job_id).as_string().unwrap_or_default(),
                    since
                ))
                .await?;
            if res.status_code() == 404 {
                yield sse(&serde_json::json!({ "type": "error", "error": "unknown job" }));
                break;
            }
            let d: serde_json::Value = res.json().await.unwrap_or(serde_json::Value::Null);
            let owner = d.get("owner").and_then(|v| v.as_str());
            // TS: d.owner && sub && d.owner !== sub  (both truthy; owner non-empty).
            if let (Some(o), Some(s)) = (owner, &sub) {
                if !o.is_empty() && o != s {
                    yield sse(&serde_json::json!({ "type": "error", "error": "forbidden" }));
                    break;
                }
            }
            if let Some(ua) = d.get("updated_at").and_then(|v| v.as_i64()) {
                since = ua;
            }
            if d.get("done").and_then(|v| v.as_bool()) == Some(true) {
                let result = d.get("result").cloned().unwrap_or(serde_json::Value::Null);
                yield sse(&serde_json::json!({ "type": "done", "result": result }));
                break;
            }
            let err = d.get("error");
            let err_truthy = err
                .map(|e| !e.is_null() && e.as_str() != Some(""))
                .unwrap_or(false);
            if err_truthy {
                yield sse(&serde_json::json!({ "type": "error", "error": err.cloned().unwrap_or(serde_json::Value::Null) }));
                break;
            }
            yield sse(&serde_json::json!({
                "type": "progress",
                "phase": d.get("phase").cloned().unwrap_or(serde_json::Value::Null),
                "thinking_tokens": d.get("thinking_tokens").and_then(|v| v.as_i64()).unwrap_or(0),
                "answer_tokens": d.get("answer_tokens").and_then(|v| v.as_i64()).unwrap_or(0),
            }));
        }
    };

    let stream: std::pin::Pin<Box<dyn futures_util::Stream<Item = Result<Vec<u8>>>>> = Box::pin(s);
    let resp = Response::from_stream(stream)?;
    let headers = resp.headers();
    let _ = headers.set("Content-Type", "text/event-stream");
    let _ = headers.set("Cache-Control", "no-cache");
    let _ = headers.set("Access-Control-Allow-Methods", "GET, POST, OPTIONS");
    let _ = headers.set("Access-Control-Allow-Headers", "Content-Type, Authorization");
    Ok(resp)
}

fn sse(o: &serde_json::Value) -> Vec<u8> {
    format!("data: {}\n\n", o).into_bytes()
}

/// RFC 4122 v4 UUID (random), matching `crypto.randomUUID()` in the TS worker.
/// Uses the JS-backed getrandom (wasm `js` feature).
fn uuid_v4() -> String {
    let mut bytes = [0u8; 16];
    getrandom::getrandom(&mut bytes).expect("getrandom failed");
    bytes[6] = (bytes[6] & 0x0f) | 0x40; // version 4
    bytes[8] = (bytes[8] & 0x3f) | 0x80; // variant 1
    format!(
        "{:02x}{:02x}{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
        bytes[0], bytes[1], bytes[2], bytes[3],
        bytes[4], bytes[5],
        bytes[6], bytes[7],
        bytes[8], bytes[9],
        bytes[10], bytes[11], bytes[12], bytes[13], bytes[14], bytes[15],
    )
}

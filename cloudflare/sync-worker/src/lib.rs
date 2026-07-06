// Cross-device data sync.
//
// Per-user data lives in a `SyncDO` (one instance per user id, `idFromName(sub)`)
// so every device authenticating with the same account hits the same dataset.
// Records merge last-writer-wins by their RFC3339 `updated_at`. Diary deletions are
// soft: the client pushes a tombstone (`deleted: true`); the DO keeps the tombstone
// so the deletion isn't resurrected by an older push, and `dump` omits it.
//
// Wire format is JSON. Auth is HS256 JWT, `sub` = user id, shared `JWT_SECRET`.

use worker::*;

mod sync_do;
mod token;
mod types;

pub use sync_do::SyncDO;

use token::validate_from_header;

// ── CORS ────────────────────────────────────────────────────────────────────
// Known origins only (no wildcard): the prod app + any renorma.app subdomain, the
// dev test env, and localhost for development. Mirrors the TS ALLOWED_ORIGIN_RE.
fn is_allowed_origin(origin: &str) -> bool {
    if origin == "https://renorma-fit-dev.pages.dev" {
        return true;
    }
    if origin == "https://renorma.app" || origin.ends_with(".renorma.app") {
        // https://([a-z0-9-]+\.)*renorma\.app
        return origin.starts_with("https://");
    }
    // http://(localhost|127.0.0.1)(:\d+)?
    if let Some(rest) = origin.strip_prefix("http://") {
        let host = rest.split(':').next().unwrap_or("");
        let port_ok = match rest.find(':') {
            None => true,
            Some(i) => rest[i + 1..].chars().all(|c| c.is_ascii_digit()) && rest[i + 1..].len() > 0,
        };
        if (host == "localhost" || host == "127.0.0.1") && port_ok {
            return true;
        }
    }
    false
}

/// Echo a matching request Origin into Access-Control-Allow-Origin and append
/// Vary: Origin. Mirrors the TS `applyCors` wrapper applied to every response.
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

/// CORS method/header allowances shared by OPTIONS and the JSON responses
/// (mirrors the TS `CORS_HEADERS`).
fn cors_base_headers(headers: &Headers) {
    let _ = headers.set("Access-Control-Allow-Methods", "GET, POST, OPTIONS");
    let _ = headers.set("Access-Control-Allow-Headers", "Content-Type, Authorization");
}

/// `new Response(body, { status, headers: { "Content-Type": json, ...CORS } })`.
fn cors_json(body: String, status: u16) -> Result<Response> {
    let headers = Headers::new();
    let _ = headers.set("Content-Type", "application/json");
    cors_base_headers(&headers);
    Ok(Response::ok(body)?.with_status(status).with_headers(headers))
}

fn error_response(message: &str, status: u16) -> Result<Response> {
    cors_json(serde_json::json!({ "error": message }).to_string(), status)
}

/// Resolve every REQUIRED Store-bound secret. On the first failure: log the full
/// reason loudly and return a 503 so ANY request to a misconfigured worker fails
/// loudly instead of degrading to a confusing 401.
async fn require_secrets(env: &Env) -> std::result::Result<(), Response> {
    for name in ["JWT_SECRET"] {
        if let Err(reason) = token::secret_or_var(env, name).await {
            console_error!("STARTUP MISCONFIG: {name}: {reason}");
            let body = format!("MISCONFIGURED: {name} — {reason}");
            return Err(
                Response::error(body, 503).unwrap_or_else(|_| Response::error("MISCONFIGURED", 503).unwrap()),
            );
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

    if req.method() == Method::Options {
        let headers = Headers::new();
        cors_base_headers(&headers);
        let resp = Response::empty()?.with_status(204).with_headers(headers);
        return apply_cors(resp, &origin);
    }

    if let Err(resp) = require_secrets(&env).await {
        return apply_cors(resp, &origin);
    }

    let resp = match handle(req, &env).await {
        Ok(r) => r,
        Err(e) => error_response(&e.to_string(), 500)?,
    };
    apply_cors(resp, &origin)
}

async fn handle(mut req: Request, env: &Env) -> Result<Response> {
    // JWT gate: 401 on missing/invalid bearer or undecodable sub.
    let user_id = match validate_from_header(&req, env).await {
        Ok(sub) => sub,
        Err(_) => return error_response("Unauthorized", 401),
    };

    let url = req.url()?;
    let path = url.path().to_string();
    let method = req.method();

    if method != Method::Post || (path != "/sync/dump" && path != "/sync/push") {
        return error_response("Not found", 404);
    }

    let body = req.text().await?;
    let stub = env
        .durable_object("SYNC_DO")?
        .id_from_name(&user_id)?
        .get_stub()?;

    let url = format!("https://do{path}");
    let headers = Headers::new();
    let _ = headers.set("Content-Type", "application/json");
    let mut init = RequestInit::new();
    init.with_method(Method::Post)
        .with_headers(headers)
        .with_body(Some(wasm_bindgen::JsValue::from_str(&body)));
    let do_req = Request::new_with_init(&url, &init)?;
    let mut res = stub.fetch_with_request(do_req).await?;
    let status = res.status_code();
    let text = res.text().await?;
    cors_json(text, status)
}

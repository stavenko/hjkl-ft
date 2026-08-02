// Cross-device data sync: a per-user journaled batch log (sync v2).
//
// Per-user data lives in a `SyncDO` (one instance per user id, `idFromName(sub)`)
// so every device authenticating with the same account hits the same journal.
// The server is dumb storage — the client forms and merges all changes; see
// sync_do.rs for the protocol (push/pull/init).
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

    // Unauthenticated liveness probe (see the frontend `net` service). Wildcard
    // CORS + before JWT/secrets so it's a cheap, always-answerable 200 from any
    // origin (incl. per-deploy Pages hash subdomains).
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
        Err(e) => error_response(&e.to_string(), 500)?,
    };
    apply_cors(resp, &origin)
}

/// Destructive surface, reachable ONLY through a service binding: a binding fetch
/// carries the dummy host the caller dialled, which no request off the internet can
/// produce (public traffic always arrives as sync.renorma.app / *.workers.dev). We
/// require that host AND the shared internal key; the caller (payment-worker) has
/// already proven the operator is an approved admin.
const INTERNAL_HOST: &str = "sync-worker";

async fn handle_internal_wipe(req: &Request, env: &Env) -> Result<Response> {
    let url = req.url()?;
    if url.host_str() != Some(INTERNAL_HOST) {
        return Response::error("Not found", 404);
    }
    let expected = match token::secret_or_var(env, "INTERNAL_PUSH_KEY").await {
        Ok(k) if !k.is_empty() => k,
        _ => return error_response("internal_not_configured", 500),
    };
    let provided = req.headers().get("X-Internal-Key").ok().flatten().unwrap_or_default();
    if provided != expected {
        return error_response("unauthorized", 403);
    }
    let user_id = url
        .query_pairs()
        .find(|(k, _)| k == "user_id")
        .map(|(_, v)| v.to_string())
        .unwrap_or_default();
    if user_id.is_empty() {
        return error_response("missing user_id", 400);
    }
    let stub = env.durable_object("SYNC_DO")?.id_from_name(&user_id)?.get_stub()?;
    let headers = Headers::new();
    let _ = headers.set("Content-Type", "application/json");
    let mut init = RequestInit::new();
    init.with_method(Method::Post).with_headers(headers);
    let do_req = Request::new_with_init("https://do/internal/user-wipe", &init)?;
    let mut res = stub.fetch_with_request(do_req).await?;
    let text = res.text().await?;
    console_log!("sync: wiped journal for {user_id}");
    cors_json(text, res.status_code())
}

async fn handle(mut req: Request, env: &Env) -> Result<Response> {
    // Erase surface first: it authenticates by binding host + internal key, NOT by
    // the user's JWT (there is no user session to erase an account with).
    if req.method() == Method::Post && req.url()?.path() == "/internal/user-wipe" {
        return handle_internal_wipe(&req, env).await;
    }
    // JWT gate: 401 on missing/invalid bearer or undecodable sub.
    let user_id = match validate_from_header(&req, env).await {
        Ok(sub) => sub,
        Err(_) => return error_response("Unauthorized", 401),
    };

    let url = req.url()?;
    let path = url.path().to_string();
    let method = req.method();

    if method != Method::Post
        || !matches!(
            path.as_str(),
            "/sync/v2/push" | "/sync/v2/pull" | "/sync/v2/init"
        )
    {
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

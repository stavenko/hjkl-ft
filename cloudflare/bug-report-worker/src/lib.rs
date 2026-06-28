// Bug-report intake.
//
// Authenticated app users file bug reports through the in-app support chat: the
// assistant gathers the details and calls its `file_bug_report` tool, which POSTs
// here. Every report is stored append-only in a single global `BugReportDO` (one
// instance, idFromName("global")), stamped with the reporting user's id (JWT `sub`)
// and a server received-at time. JWT-gated, so only signed-in app users can reach
// it. The admin read (GET /reports) is gated by ADMIN_KEY, not a user JWT.

use wasm_bindgen::JsValue;
use worker::*;

mod bug_report_do;
mod token;

pub use bug_report_do::BugReportDO;

// ── CORS ────────────────────────────────────────────────────────────────────
// Known origins only (no wildcard): the prod app + any renorma.app subdomain, the
// dev test env, and localhost for development. Mirrors the TS ALLOWED_ORIGIN_RE.
fn is_allowed_origin(origin: &str) -> bool {
    origin == "https://renorma.app"
        || (origin.starts_with("https://") && origin.ends_with(".renorma.app"))
        || origin == "https://hjkl-ft.pages.dev"
        || origin.starts_with("http://localhost")
        || origin.starts_with("http://127.0.0.1")
}

/// CORS headers attached to every JSON response (matches TS CORS_HEADERS). The
/// Access-Control-Allow-Origin / Vary:Origin is added by `apply_cors` at the edge.
fn cors_method_headers(headers: &Headers) {
    let _ = headers.set("Access-Control-Allow-Methods", "GET, POST, OPTIONS");
    let _ = headers.set("Access-Control-Allow-Headers", "Content-Type, Authorization, X-Admin-Key");
}

/// Echo a matching request Origin into Access-Control-Allow-Origin (no wildcard).
/// Always appends Vary: Origin. Mirrors the TS `applyCors`.
fn apply_cors(resp: Response, origin: &str) -> Result<Response> {
    let headers = Headers::new();
    for (k, v) in resp.headers() {
        let _ = headers.set(&k, &v);
    }
    let _ = headers.append("Vary", "Origin");
    if !origin.is_empty() && is_allowed_origin(origin) {
        let _ = headers.set("Access-Control-Allow-Origin", origin);
    }
    let status = resp.status_code();
    Ok(Response::from_body(resp.body().clone())?
        .with_headers(headers)
        .with_status(status))
}

// ── error helpers ─────────────────────────────────────────────────────────────
/// `{ "error": <message> }` with status + CORS method headers, mirroring the TS
/// `errorResponse` (which wraps `corsJson`).
fn error_response(message: &str, status: u16) -> Response {
    let resp = Response::from_json(&serde_json::json!({ "error": message }))
        .expect("serialize error")
        .with_status(status);
    let headers = resp.headers();
    cors_method_headers(headers);
    resp
}

/// Relay a DO response body + status, setting Content-Type + CORS method headers
/// (mirrors the TS `corsJson(await res.text(), res.status)`).
async fn cors_relay(mut res: Response) -> Result<Response> {
    let status = res.status_code();
    let text = res.text().await?;
    let headers = Headers::new();
    let _ = headers.set("Content-Type", "application/json");
    cors_method_headers(&headers);
    Ok(Response::ok(text)?.with_status(status).with_headers(headers))
}

// ── DO stub ────────────────────────────────────────────────────────────────────
fn bug_stub(env: &Env) -> Result<worker::durable::Stub> {
    env.durable_object("BUG_REPORT_DO")?
        .id_from_name("global")?
        .get_stub()
}

async fn do_get(stub: &worker::durable::Stub, path: &str) -> Result<Response> {
    stub.fetch_with_str(&format!("https://do{path}")).await
}

async fn do_post(
    stub: &worker::durable::Stub,
    path: &str,
    body: &serde_json::Value,
) -> Result<Response> {
    let body_str = serde_json::to_string(body)
        .map_err(|e| Error::RustError(format!("serialize DO body: {e}")))?;
    let headers = Headers::new();
    headers
        .set("Content-Type", "application/json")
        .map_err(|e| Error::RustError(format!("set header: {e}")))?;
    let mut init = RequestInit::new();
    init.with_method(Method::Post)
        .with_headers(headers)
        .with_body(Some(JsValue::from_str(&body_str)));
    let req = Request::new_with_init(&format!("https://do{path}"), &init)?;
    stub.fetch_with_request(req).await
}

// ── fail-loud secrets ──────────────────────────────────────────────────────────
/// Resolve every REQUIRED secret at the top of the fetch entry. On the first
/// failure: log loudly and return 503 so ANY request makes the misconfiguration
/// obvious (Workers have no separate startup — per-request is intended). Mirrors the
/// TS `requireSecrets` over the same names.
async fn require_secrets(env: &Env) -> std::result::Result<(), Response> {
    for name in ["JWT_SECRET", "ADMIN_KEY"] {
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

    // Preflight short-circuit (matches TS: 204 with CORS_HEADERS, before secrets).
    if req.method() == Method::Options {
        let headers = Headers::new();
        cors_method_headers(&headers);
        return apply_cors(Response::empty()?.with_headers(headers).with_status(204), &origin);
    }

    if let Err(resp) = require_secrets(&env).await {
        // The TS does NOT route the 503 through applyCors (requireSecrets returns a
        // bare Response inside inner.fetch, which IS wrapped by applyCors). Match
        // that: wrap so a matching origin still gets ACAO.
        return apply_cors(resp, &origin);
    }

    let resp = match handle(req, &env).await {
        Ok(r) => r,
        Err(e) => error_response(&e.to_string(), 500),
    };
    apply_cors(resp, &origin)
}

async fn handle(mut req: Request, env: &Env) -> Result<Response> {
    let url = req.url()?;
    let path = url.path().to_string();
    let method = req.method();

    let stub = bug_stub(env)?;

    // ── Admin read: gather the collected reports. Gated by ADMIN_KEY (a developer
    // tool), NOT a user JWT — so one signed-in user can't read others' reports. ──
    if method == Method::Get && path == "/reports" {
        let admin_key = req
            .headers()
            .get("X-Admin-Key")
            .ok()
            .flatten()
            .unwrap_or_default();
        let expected = match token::secret_or_var(env, "ADMIN_KEY").await {
            Ok(k) => k,
            Err(reason) => {
                console_error!("ADMIN_KEY resolve failed: {reason}");
                return Ok(error_response(&format!("MISCONFIGURED: ADMIN_KEY — {reason}"), 503));
            }
        };
        if admin_key != expected {
            return Ok(error_response("Unauthorized", 401));
        }
        let res = do_get(&stub, "/reports").await?;
        return cors_relay(res).await;
    }

    // ── Everything else is app-JWT authed ──
    let auth_header = req
        .headers()
        .get("Authorization")
        .ok()
        .flatten()
        .unwrap_or_default();
    let bearer = auth_header.strip_prefix("Bearer ").unwrap_or("").to_string();

    let secret = match token::secret_or_var(env, "JWT_SECRET").await {
        Ok(s) => s,
        Err(reason) => {
            console_error!("JWT_SECRET resolve failed: {reason}");
            return Ok(error_response(&format!("MISCONFIGURED: JWT_SECRET — {reason}"), 503));
        }
    };

    if bearer.is_empty() || !token::verify_jwt(&bearer, &secret) {
        return Ok(error_response("Unauthorized", 401));
    }
    let user_id = match token::decode_jwt_sub(&bearer) {
        Some(u) => u,
        None => return Ok(error_response("Unauthorized", 401)),
    };

    if method == Method::Post && path == "/report" {
        let mut body: serde_json::Value = req.json().await?;
        if let Some(obj) = body.as_object_mut() {
            obj.insert("user".to_string(), serde_json::Value::String(user_id));
        }
        let res = do_post(&stub, "/report", &body).await?;
        return cors_relay(res).await;
    }

    Ok(error_response("Not found", 404))
}

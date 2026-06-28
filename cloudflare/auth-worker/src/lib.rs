use worker::*;

mod auth;
mod auth_do;
mod pair;

mod recovery;
mod token;
mod types;
mod user_do;

pub use auth_do::AuthDO;
pub use user_do::UserDO;

/// Get the global AuthDO stub (single instance for all auth operations).
pub(crate) fn auth_do_stub(env: &Env) -> Result<worker::durable::Stub> {
    let namespace = env.durable_object("AUTH_DO")?;
    namespace.id_from_name("global")?.get_stub()
}

/// Read the browser Origin header from a request (empty string if absent).
pub(crate) fn request_origin(req: &Request) -> String {
    req.headers().get("Origin").ok().flatten().unwrap_or_default()
}

/// Build an internal POST request to a Durable Object with the given path and JSON body.
pub(crate) fn do_request(path: &str, body: &serde_json::Value) -> Result<Request> {
    let url = format!("https://internal{path}");
    let body_str = serde_json::to_string(body)
        .map_err(|e| Error::RustError(format!("serialize DO request: {e}")))?;
    Request::new_with_init(
        &url,
        RequestInit::new()
            .with_method(Method::Post)
            .with_body(Some(wasm_bindgen::JsValue::from_str(&body_str))),
    )
}

/// Known origins only (no wildcard): the prod app + any renorma.app subdomain,
/// the dev test env, and localhost for development.
fn is_allowed_origin(origin: &str) -> bool {
    origin == "https://renorma.app"
        || (origin.starts_with("https://") && origin.ends_with(".renorma.app"))
        || origin == "https://renorma-fit-dev.pages.dev"
        || origin == "https://renorma-admin-dev.pages.dev"
        || origin.starts_with("http://localhost")
        || origin.starts_with("http://127.0.0.1")
}

fn add_cors(resp: Response, origin: &str) -> Result<Response> {
    let mut headers = Headers::new();
    if is_allowed_origin(origin) {
        let _ = headers.set("Access-Control-Allow-Origin", origin);
    }
    let _ = headers.set("Vary", "Origin");
    let _ = headers.set("Access-Control-Allow-Methods", "GET, POST, OPTIONS");
    let _ = headers.set("Access-Control-Allow-Headers", "Content-Type, Authorization");
    // Copy original headers
    for (k, v) in resp.headers() {
        let _ = headers.set(&k, &v);
    }
    let status = resp.status_code();
    Ok(Response::from_body(resp.body().clone())?.with_headers(headers).with_status(status))
}

/// Resolve every REQUIRED Store-bound secret at the top of the fetch entry. On
/// the first failure, log the full reason loudly and return a 503 so that ANY
/// request immediately shows the worker is misconfigured and why.
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
    // Capture the request Origin before the router consumes `req`.
    let origin = req.headers().get("Origin").ok().flatten().unwrap_or_default();

    if req.method() == Method::Options {
        let mut headers = Headers::new();
        if is_allowed_origin(&origin) {
            let _ = headers.set("Access-Control-Allow-Origin", &origin);
        }
        let _ = headers.set("Vary", "Origin");
        let _ = headers.set("Access-Control-Allow-Methods", "GET, POST, OPTIONS");
        let _ = headers.set("Access-Control-Allow-Headers", "Content-Type, Authorization");
        let _ = headers.set("Access-Control-Max-Age", "86400");
        return Ok(Response::empty()?.with_headers(headers).with_status(204));
    }

    if let Err(resp) = require_secrets(&env).await {
        return Ok(resp);
    }

    let router = Router::new();

    let result = router
        .post_async("/register/begin", auth::register_begin)
        .post_async("/register/finish", auth::register_finish)
        .post_async("/authenticate/begin", auth::authenticate_begin)
        .post_async("/authenticate/finish", auth::authenticate_finish)
        .post_async("/add-device/begin", auth::add_device_begin)
        .post_async("/add-device/finish", auth::add_device_finish)
        .post_async("/recovery/set", recovery::set_recovery_key)
        .post_async("/recovery/authenticate", recovery::authenticate_with_recovery)
        .post_async("/pair/create", pair::create_pairing)
        .post_async("/pair/request", pair::request_pairing)
        .post_async("/pair/approve", pair::approve_pairing)
        .post_async("/pair/check", pair::check_pairing)
        .post_async("/pair/claim", pair::claim_pairing)
        .post_async("/pair/finish", pair::finish_pairing)
        .get_async("/pair/status/:id", pair::pairing_status)
        .post_async("/token/validate", token::validate_token)
        .get_async("/tokens", token::list_tokens)
        .run(req, env)
        .await;

    match result {
        Ok(resp) => add_cors(resp, &origin),
        Err(e) => {
            let body = serde_json::json!({ "error": e.to_string() });
            let mut resp = Response::from_json(&body)?.with_status(500);
            let headers = resp.headers_mut();
            if is_allowed_origin(&origin) {
                let _ = headers.set("Access-Control-Allow-Origin", &origin);
            }
            let _ = headers.set("Vary", "Origin");
            Ok(resp)
        }
    }
}


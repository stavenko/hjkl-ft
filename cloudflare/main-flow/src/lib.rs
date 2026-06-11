use worker::*;
use serde::{Deserialize, Serialize};

mod push_do;
mod push_send;
mod schedule_do;

pub use push_do::PushDO;
pub use schedule_do::ScheduleDO;

fn add_cors(resp: Response) -> Result<Response> {
    let mut headers = Headers::new();
    let _ = headers.set("Access-Control-Allow-Origin", "*");
    let _ = headers.set("Access-Control-Allow-Methods", "GET, POST, OPTIONS");
    let _ = headers.set("Access-Control-Allow-Headers", "Content-Type, Authorization");
    for (k, v) in resp.headers() {
        let _ = headers.set(&k, &v);
    }
    let status = resp.status_code();
    Ok(Response::from_body(resp.body().clone())?.with_headers(headers).with_status(status))
}

fn push_do_stub(env: &Env) -> Result<worker::durable::Stub> {
    let namespace = env.durable_object("PUSH_DO")?;
    namespace.id_from_name("global")?.get_stub()
}

fn schedule_do_stub(env: &Env, user_id: &str) -> Result<worker::durable::Stub> {
    let namespace = env.durable_object("SCHEDULE_DO")?;
    namespace.id_from_name(user_id)?.get_stub()
}

fn do_request(path: &str, body: &serde_json::Value) -> Result<Request> {
    let url = format!("https://internal{path}");
    let body_str = serde_json::to_string(body)
        .map_err(|e| Error::RustError(format!("serialize: {e}")))?;
    Request::new_with_init(
        &url,
        RequestInit::new()
            .with_method(Method::Post)
            .with_body(Some(wasm_bindgen::JsValue::from_str(&body_str))),
    )
}

// ---------------------------------------------------------------------------
// HTTP endpoints
// ---------------------------------------------------------------------------

async fn vapid_key(_req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let public_key = ctx.env.var("VAPID_PUBLIC_KEY")
        .map(|v| v.to_string())
        .map_err(|_| Error::RustError("VAPID_PUBLIC_KEY not configured".into()))?;
    Response::from_json(&serde_json::json!({ "public_key": public_key }))
}

async fn subscribe(mut req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let user_id = validate_token(&req, &ctx)?;
    let subscription: PushSubscription = req.json().await
        .map_err(|e| Error::RustError(format!("invalid body: {e}")))?;

    let stub = push_do_stub(&ctx.env)?;
    let do_body = serde_json::json!({
        "user_id": user_id,
        "subscription": subscription,
    });
    let internal_req = do_request("/subscribe", &do_body)?;
    stub.fetch_with_request(internal_req).await
}

async fn unsubscribe(mut req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let user_id = validate_token(&req, &ctx)?;
    let body: serde_json::Value = req.json().await
        .map_err(|e| Error::RustError(format!("invalid body: {e}")))?;
    let endpoint = body.get("endpoint").and_then(|v| v.as_str())
        .ok_or_else(|| Error::RustError("missing endpoint".into()))?;

    let stub = push_do_stub(&ctx.env)?;
    let do_body = serde_json::json!({
        "user_id": user_id,
        "endpoint": endpoint,
    });
    let internal_req = do_request("/unsubscribe", &do_body)?;
    stub.fetch_with_request(internal_req).await
}

async fn update_schedule(mut req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let user_id = validate_token(&req, &ctx)?;
    let schedule: serde_json::Value = req.json().await
        .map_err(|e| Error::RustError(format!("invalid body: {e}")))?;

    let stub = schedule_do_stub(&ctx.env, &user_id)?;
    let do_body = serde_json::json!({
        "user_id": user_id,
        "schedule": schedule,
    });
    let internal_req = do_request("/update", &do_body)?;
    stub.fetch_with_request(internal_req).await
}

async fn test_alarm(mut req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let body: serde_json::Value = req.json().await
        .map_err(|e| Error::RustError(format!("invalid body: {e}")))?;
    let user_id = body.get("user_id").and_then(|v| v.as_str()).unwrap_or("test-user");
    let stub = schedule_do_stub(&ctx.env, user_id)?;
    let internal_req = do_request("/test-alarm", &body)?;
    stub.fetch_with_request(internal_req).await
}

async fn get_schedule(req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let user_id = validate_token(&req, &ctx)?;
    let stub = schedule_do_stub(&ctx.env, &user_id)?;
    let url = "https://internal/get";
    let internal_req = Request::new(url, Method::Get)?;
    stub.fetch_with_request(internal_req).await
}

async fn broadcast(mut req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let body: serde_json::Value = req.json().await
        .map_err(|e| Error::RustError(format!("invalid body: {e}")))?;
    let payload = body.get("payload").and_then(|v| v.as_str())
        .unwrap_or(r#"{"title":"Food Tracker","body":"Время записать приём пищи!","icon":"/icon-192.png","tag":"reminder","url":"/"}"#);

    send_to_all(&ctx.env, payload).await
}

// ---------------------------------------------------------------------------
// Token validation (calls auth-worker)
// ---------------------------------------------------------------------------

fn validate_token(req: &Request, ctx: &RouteContext<()>) -> Result<String> {
    let auth_header = req.headers().get("Authorization")
        .map_err(|e| Error::RustError(format!("{e}")))?
        .ok_or_else(|| Error::RustError("missing Authorization header".into()))?;
    let token = auth_header.strip_prefix("Bearer ")
        .ok_or_else(|| Error::RustError("invalid Authorization header".into()))?;

    let secret = ctx.env.var("JWT_SECRET")
        .map(|v| v.to_string())
        .map_err(|_| Error::RustError("JWT_SECRET not configured".into()))?;

    verify_jwt(token, &secret)
}

fn verify_jwt(token: &str, secret: &str) -> Result<String> {
    use hmac::{Hmac, Mac};
    use sha2::Sha256;
    use base64::Engine;

    let parts: Vec<&str> = token.split('.').collect();
    if parts.len() != 3 {
        return Err(Error::RustError("invalid JWT".into()));
    }

    let signing_input = format!("{}.{}", parts[0], parts[1]);
    let mut mac = Hmac::<Sha256>::new_from_slice(secret.as_bytes())
        .map_err(|e| Error::RustError(format!("hmac: {e}")))?;
    mac.update(signing_input.as_bytes());

    let sig = base64::engine::general_purpose::URL_SAFE_NO_PAD.decode(parts[2])
        .map_err(|e| Error::RustError(format!("decode sig: {e}")))?;
    mac.verify_slice(&sig)
        .map_err(|_| Error::RustError("invalid JWT signature".into()))?;

    let payload = base64::engine::general_purpose::URL_SAFE_NO_PAD.decode(parts[1])
        .map_err(|e| Error::RustError(format!("decode payload: {e}")))?;
    let claims: serde_json::Value = serde_json::from_slice(&payload)
        .map_err(|e| Error::RustError(format!("parse claims: {e}")))?;

    claims.get("sub").and_then(|v| v.as_str()).map(|s| s.to_string())
        .ok_or_else(|| Error::RustError("missing sub in JWT".into()))
}

// ---------------------------------------------------------------------------
// Send push to a specific user's devices
// ---------------------------------------------------------------------------

pub async fn send_push_to_user(env: &Env, user_id: &str, payload: &str) -> Result<()> {
    let vapid_private_b64 = env.var("VAPID_PRIVATE_KEY").map(|v| v.to_string())
        .map_err(|e| Error::RustError(format!("VAPID_PRIVATE_KEY: {e}")))?;
    let vapid_public_b64 = env.var("VAPID_PUBLIC_KEY").map(|v| v.to_string())
        .map_err(|e| Error::RustError(format!("VAPID_PUBLIC_KEY: {e}")))?;
    let vapid_subject = env.var("VAPID_SUBJECT").map(|v| v.to_string())
        .map_err(|e| Error::RustError(format!("VAPID_SUBJECT: {e}")))?;

    let vapid_private = push_send::b64url_decode(&vapid_private_b64)
        .map_err(|e| Error::RustError(format!("decode private key: {e}")))?;
    let vapid_public = push_send::b64url_decode(&vapid_public_b64)
        .map_err(|e| Error::RustError(format!("decode public key: {e}")))?;

    let stub = push_do_stub(env)?;
    let req = do_request("/list", &serde_json::json!({"user_id": user_id}))?;
    let mut resp = stub.fetch_with_request(req).await?;
    if resp.status_code() != 200 {
        return Err(Error::RustError(format!("PushDO /list returned {}", resp.status_code())));
    }

    let subs: Vec<PushSubscription> = resp.json().await
        .map_err(|e| Error::RustError(format!("parse subscriptions: {e}")))?;

    for sub in &subs {
        match push_send::send_push(sub, payload, &vapid_private, &vapid_public, &vapid_subject).await {
            Ok(true) => {}
            Ok(false) => {
                let remove_stub = push_do_stub(env)?;
                let remove_req = do_request("/unsubscribe-by-endpoint", &serde_json::json!({"endpoint": sub.endpoint}))?;
                let _ = remove_stub.fetch_with_request(remove_req).await;
            }
            Err(e) => {
                console_log!("Push to user {} failed: {}", user_id, e);
            }
        }
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Send push to all subscribers
// ---------------------------------------------------------------------------

async fn send_to_all(env: &Env, payload: &str) -> Result<Response> {
    let vapid_private_b64 = env.var("VAPID_PRIVATE_KEY").map(|v| v.to_string())
        .map_err(|e| Error::RustError(format!("VAPID_PRIVATE_KEY: {e}")))?;
    let vapid_public_b64 = env.var("VAPID_PUBLIC_KEY").map(|v| v.to_string())
        .map_err(|e| Error::RustError(format!("VAPID_PUBLIC_KEY: {e}")))?;
    let vapid_subject = env.var("VAPID_SUBJECT").map(|v| v.to_string())
        .map_err(|e| Error::RustError(format!("VAPID_SUBJECT: {e}")))?;

    let vapid_private = push_send::b64url_decode(&vapid_private_b64)
        .map_err(|e| Error::RustError(format!("decode private key: {e}")))?;
    let vapid_public = push_send::b64url_decode(&vapid_public_b64)
        .map_err(|e| Error::RustError(format!("decode public key: {e}")))?;

    let stub = push_do_stub(env)?;
    let req = do_request("/list-all", &serde_json::json!({}))?;
    let mut resp = stub.fetch_with_request(req).await?;
    if resp.status_code() != 200 {
        return Err(Error::RustError(format!("DO /list-all returned {}", resp.status_code())));
    }

    let subs: Vec<PushSubscription> = resp.json().await
        .map_err(|e| Error::RustError(format!("parse subscriptions: {e}")))?;

    let mut sent = 0u32;
    let mut failed = 0u32;
    let mut gone = 0u32;

    for sub in &subs {
        match push_send::send_push(sub, payload, &vapid_private, &vapid_public, &vapid_subject).await {
            Ok(true) => sent += 1,
            Ok(false) => {
                gone += 1;
                let remove_body = serde_json::json!({
                    "user_id": "_cleanup",
                    "endpoint": sub.endpoint,
                });
                let remove_stub = push_do_stub(env)?;
                let remove_req = do_request("/unsubscribe-by-endpoint", &remove_body)?;
                let _ = remove_stub.fetch_with_request(remove_req).await;
            }
            Err(_) => failed += 1,
        }
    }

    console_log!("Push broadcast: sent={sent}, failed={failed}, gone={gone}, total={}", subs.len());
    Response::from_json(&serde_json::json!({ "sent": sent, "failed": failed, "gone": gone }))
}

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PushSubscription {
    pub endpoint: String,
    pub keys: PushSubscriptionKeys,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PushSubscriptionKeys {
    pub p256dh: String,
    pub auth: String,
}

// ---------------------------------------------------------------------------
// Entrypoints
// ---------------------------------------------------------------------------

#[event(fetch)]
async fn main(req: Request, env: Env, _ctx: Context) -> Result<Response> {
    if req.method() == Method::Options {
        let mut headers = Headers::new();
        let _ = headers.set("Access-Control-Allow-Origin", "*");
        let _ = headers.set("Access-Control-Allow-Methods", "GET, POST, OPTIONS");
        let _ = headers.set("Access-Control-Allow-Headers", "Content-Type, Authorization");
        let _ = headers.set("Access-Control-Max-Age", "86400");
        return Ok(Response::empty()?.with_headers(headers).with_status(204));
    }

    let router = Router::new();
    let result = router
        .get_async("/push/vapid-key", vapid_key)
        .post_async("/push/subscribe", subscribe)
        .post_async("/push/unsubscribe", unsubscribe)
        .post_async("/push/broadcast", broadcast)
        .post_async("/schedule", update_schedule)
        .get_async("/schedule", get_schedule)
        .post_async("/test-alarm", test_alarm)
        .run(req, env)
        .await;

    match result {
        Ok(resp) => add_cors(resp),
        Err(e) => {
            let body = serde_json::json!({ "error": e.to_string() });
            let mut resp = Response::from_json(&body)?.with_status(500);
            let headers = resp.headers_mut();
            let _ = headers.set("Access-Control-Allow-Origin", "*");
            Ok(resp)
        }
    }
}


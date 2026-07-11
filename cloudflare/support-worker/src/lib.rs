use worker::*;

mod conversation_do;
mod conversation_index_do;
mod token;
mod types;

pub use conversation_do::ConversationDO;
pub use conversation_index_do::ConversationIndexDO;

use token::validate_from_header;
use types::{AppendResult, ErrorResponse};

const PREVIEW_MAX: usize = 200;

// ---- Durable Object stubs ----

fn conversation_stub(env: &Env, user_id: &str) -> Result<worker::durable::Stub> {
    env.durable_object("CONVERSATION_DO")?
        .id_from_name(user_id)?
        .get_stub()
}

fn index_stub(env: &Env) -> Result<worker::durable::Stub> {
    env.durable_object("CONVERSATION_INDEX_DO")?
        .id_from_name("index")?
        .get_stub()
}

/// Build an internal POST request to a Durable Object with the given path and JSON body.
fn do_request(path: &str, body: &serde_json::Value) -> Result<Request> {
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

// ---- error helpers ----

fn json_status(status: u16, message: &str) -> Response {
    let body = ErrorResponse {
        error: message.to_string(),
    };
    Response::from_json(&body)
        .expect("serialize ErrorResponse")
        .with_status(status)
}

// ---- auth gates ----

/// 401 on any signature/format failure; returns the authenticated user_id (sub).
async fn auth_user(req: &Request, env: &Env) -> std::result::Result<String, Response> {
    validate_from_header(req, env)
        .await
        .map_err(|e| json_status(401, &e.to_string()))
}

/// Operator-only secret for POST /admin/approve (X-Admin-Secret). Read like
/// INTERNAL_PUSH_KEY: env.secret first (prod `wrangler secret put`), env.var
/// fallback (dev [vars]). Err means UNSET — the caller MUST fail closed.
async fn admin_approve_secret(env: &Env) -> std::result::Result<String, String> {
    token::secret_or_var(env, "ADMIN_APPROVE_SECRET").await
}

/// 401 if the token is invalid, 403 if a valid sub is not DO-approved. Returns
/// the expert's sub on success.
///
/// The ONLY source of truth is the GLOBAL index DO's `admins` table (runtime-
/// mutable via the approve flow, no redeploy). On ANY DO/stub/parse failure we
/// 500 (fail loudly); there is NO code path that grants expert access without a
/// stored approval.
async fn auth_expert(req: &Request, env: &Env) -> std::result::Result<String, Response> {
    let sub = validate_from_header(req, env)
        .await
        .map_err(|e| json_status(401, &e.to_string()))?;
    let do_req = match do_request("/admin-is-approved", &serde_json::json!({ "sub": sub })) {
        Ok(r) => r,
        Err(e) => return Err(json_status(500, &format!("admin auth: {e}"))),
    };
    let stub = match index_stub(env) {
        Ok(s) => s,
        Err(e) => return Err(json_status(500, &format!("admin auth stub: {e}"))),
    };
    let mut resp = match stub.fetch_with_request(do_req).await {
        Ok(r) => r,
        Err(e) => return Err(json_status(500, &format!("admin auth fetch: {e}"))),
    };
    if resp.status_code() != 200 {
        return Err(json_status(500, "admin auth DO error"));
    }
    let v: serde_json::Value = match resp.json().await {
        Ok(v) => v,
        Err(e) => return Err(json_status(500, &format!("admin auth parse: {e}"))),
    };
    if v.get("approved").and_then(|b| b.as_bool()).unwrap_or(false) {
        Ok(sub)
    } else {
        Err(json_status(403, "not an expert"))
    }
}

fn truncate_preview(text: &str) -> String {
    text.chars().take(PREVIEW_MAX).collect()
}

/// Server-to-server push: nudge `user_id` to re-open the Live chat after an
/// expert reply. Delegates to main-flow's `/push/notify` (the only worker that
/// holds the VAPID keys + the user's push subscriptions), authenticated by the
/// shared `INTERNAL_PUSH_KEY` — the SAME contract payment-worker already uses.
///
/// The deep-link `url` follows the notification convention: `?notif=1` tells the
/// app this navigation came from a tapped push.
///
/// Returns Err on any misconfiguration or non-2xx from main-flow. This is a
/// BEST-EFFORT nudge: the caller (expert_reply) logs the error loudly but does
/// NOT fail the reply on it (the reply is already committed) — matching
/// payment-worker's notifyPush policy.
async fn nudge_user_push(env: &Env, user_id: &str, text: &str) -> Result<()> {
    let key = token::secret_or_var(env, "INTERNAL_PUSH_KEY")
        .await
        .map_err(Error::RustError)?;

    let body = serde_json::json!({
        "userId": user_id,
        "body": truncate_preview(text),
        "url": "/chat?notif=1",
    })
    .to_string();

    let headers = Headers::new();
    headers
        .set("Content-Type", "application/json")
        .map_err(|e| Error::RustError(format!("set header: {e}")))?;
    headers
        .set("X-Internal-Key", &key)
        .map_err(|e| Error::RustError(format!("set header: {e}")))?;

    let mut init = RequestInit::new();
    init.with_method(Method::Post)
        .with_headers(headers)
        .with_body(Some(wasm_bindgen::JsValue::from_str(&body)));

    // The host is irrelevant for a service-binding fetch; only the path routes
    // inside main-flow. Bind-fetch avoids the workers.dev Worker→Worker 404.
    let request = Request::new_with_init("https://main-flow/push/notify", &init)
        .map_err(|e| Error::RustError(format!("build push request: {e}")))?;

    let main_flow = env
        .service("MAIN_FLOW")
        .map_err(|e| Error::RustError(format!("MAIN_FLOW service binding: {e}")))?;
    let resp = main_flow
        .fetch_request(request)
        .await
        .map_err(|e| Error::RustError(format!("push notify fetch failed: {e}")))?;

    let status = resp.status_code();
    if !(200..300).contains(&status) {
        return Err(Error::RustError(format!(
            "push notify returned {status} for user {user_id}"
        )));
    }
    Ok(())
}

/// Parse an AppendResult from a DO response.
async fn read_append(resp: &mut Response) -> Result<AppendResult> {
    if resp.status_code() != 200 {
        return Err(Error::RustError(format!(
            "conversation DO append failed: {}",
            resp.status_code()
        )));
    }
    resp.json().await
}

// ---- USER handlers ----

async fn user_send(mut req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let uid = match auth_user(&req, &ctx.env).await {
        Ok(s) => s,
        Err(resp) => return Ok(resp),
    };

    let body: serde_json::Value = req.json().await?;
    let client_id = body.get("client_id").and_then(|v| v.as_str()).unwrap_or("");
    let text = body.get("text").and_then(|v| v.as_str()).unwrap_or("");
    if client_id.is_empty() || text.is_empty() {
        return Ok(json_status(400, "client_id and text are required"));
    }
    // Typed envelope (default kind='text'). `payload` is a RAW JSON string;
    // forwarded verbatim to the DO. See the data-request/data-share protocol.
    let (kind, payload) = typed_envelope(&body);

    let append_req = do_request(
        "/append",
        &serde_json::json!({
            "client_id": client_id,
            "text": text,
            "sender": "user",
            "kind": kind,
            "payload": payload,
        }),
    )?;
    let mut do_resp = conversation_stub(&ctx.env, &uid)?
        .fetch_with_request(append_req)
        .await?;
    let result = read_append(&mut do_resp).await?;

    // Index maintenance — ALWAYS call it, even on a deduped (retried) append.
    // touch-user is idempotent + monotonic (a deduped/older seq is a no-op), so a
    // retry self-heals a previously-failed index touch instead of corrupting the
    // queue. We still fail loudly on a genuine index error so the client retries.
    let touch_req = do_request(
        "/touch-user",
        &serde_json::json!({
            "user_id": uid,
            "preview": truncate_preview(text),
            "last_ts": result.created_at,
            "last_seq": result.seq,
        }),
    )?;
    let touch_resp = index_stub(&ctx.env)?.fetch_with_request(touch_req).await?;
    if touch_resp.status_code() != 200 {
        return Err(Error::RustError("index touch-user failed".into()));
    }

    Response::from_json(&serde_json::json!({ "seq": result.seq, "created_at": result.created_at }))
}

async fn user_messages(req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let uid = match auth_user(&req, &ctx.env).await {
        Ok(s) => s,
        Err(resp) => return Ok(resp),
    };
    let (after_seq, limit) = parse_paging(&req)?;
    let wait_ms = parse_wait_ms(&req);
    let list_req = do_request(
        "/list",
        &serde_json::json!({ "after_seq": after_seq, "limit": limit, "wait_ms": wait_ms }),
    )?;
    conversation_stub(&ctx.env, &uid)?
        .fetch_with_request(list_req)
        .await
}

async fn user_read(mut req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let uid = match auth_user(&req, &ctx.env).await {
        Ok(s) => s,
        Err(resp) => return Ok(resp),
    };
    let body: serde_json::Value = req.json().await?;
    let seq = body
        .get("seq")
        .and_then(|v| v.as_i64())
        .ok_or_else(|| Error::RustError("missing seq".into()))?;
    let read_req = do_request("/read", &serde_json::json!({ "who": "user", "seq": seq }))?;
    conversation_stub(&ctx.env, &uid)?
        .fetch_with_request(read_req)
        .await
}

// ---- EXPERT handlers ----

async fn expert_conversations(req: Request, ctx: RouteContext<()>) -> Result<Response> {
    if let Err(resp) = auth_expert(&req, &ctx.env).await {
        return Ok(resp);
    }
    let url = req.url()?;
    let q: std::collections::HashMap<_, _> = url.query_pairs().into_owned().collect();
    let status = q.get("status").map(|s| s.as_str()).unwrap_or("pending");
    let after = q.get("after").map(|s| s.as_str());
    let limit: i64 = q.get("limit").and_then(|s| s.parse().ok()).unwrap_or(50);

    let mut body = serde_json::json!({ "status": status, "limit": limit });
    if let Some(a) = after {
        body["after"] = serde_json::json!(a);
    }
    let req = do_request("/conversations", &body)?;
    index_stub(&ctx.env)?.fetch_with_request(req).await
}

async fn expert_messages(req: Request, ctx: RouteContext<()>) -> Result<Response> {
    if let Err(resp) = auth_expert(&req, &ctx.env).await {
        return Ok(resp);
    }
    let uid = ctx
        .param("uid")
        .ok_or_else(|| Error::RustError("missing uid".into()))?
        .clone();
    let (after_seq, limit) = parse_paging(&req)?;
    let wait_ms = parse_wait_ms(&req);
    let list_req = do_request(
        "/list",
        &serde_json::json!({ "after_seq": after_seq, "limit": limit, "wait_ms": wait_ms }),
    )?;
    conversation_stub(&ctx.env, &uid)?
        .fetch_with_request(list_req)
        .await
}

async fn expert_reply(mut req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let expert_sub = match auth_expert(&req, &ctx.env).await {
        Ok(s) => s,
        Err(resp) => return Ok(resp),
    };
    let uid = ctx
        .param("uid")
        .ok_or_else(|| Error::RustError("missing uid".into()))?
        .clone();

    let body: serde_json::Value = req.json().await?;
    let client_id = body.get("client_id").and_then(|v| v.as_str()).unwrap_or("");
    let text = body.get("text").and_then(|v| v.as_str()).unwrap_or("");
    if client_id.is_empty() || text.is_empty() {
        return Ok(json_status(400, "client_id and text are required"));
    }
    // Typed envelope (default kind='text'). A curator data-request rides here as
    // kind='data_request' with the {dataset} payload; forwarded verbatim.
    let (kind, payload) = typed_envelope(&body);

    let append_req = do_request(
        "/append",
        &serde_json::json!({
            "client_id": client_id,
            "text": text,
            "sender": "expert",
            "expert_id": expert_sub,
            "kind": kind,
            "payload": payload,
        }),
    )?;
    let mut do_resp = conversation_stub(&ctx.env, &uid)?
        .fetch_with_request(append_req)
        .await?;
    let result = read_append(&mut do_resp).await?;

    // Index maintenance — ALWAYS call it, even on a deduped (retried) reply.
    // clear-pending is idempotent + monotonic: it clears pending ONLY IF no newer
    // user message arrived after this reply's seq (existing last_seq <= reply_seq).
    // So a retried/stale reply can't drop a conversation the user re-opened, and
    // a previously-failed clear self-heals on retry. `last_seq` carries THIS reply's
    // seq, used by the DO as reply_seq.
    let clear_req = do_request(
        "/clear-pending",
        &serde_json::json!({
            "user_id": uid,
            "preview": truncate_preview(text),
            "last_ts": result.created_at,
            "last_seq": result.seq,
        }),
    )?;
    let clear_resp = index_stub(&ctx.env)?.fetch_with_request(clear_req).await?;
    if clear_resp.status_code() != 200 {
        return Err(Error::RustError("index clear-pending failed".into()));
    }

    // Nudge the user to re-open the Live chat. BEST-EFFORT (same contract +
    // policy as payment-worker's notifyPush): the reply is already committed +
    // indexed above, so a push failure MUST NOT fail the reply — it is logged
    // loudly (never swallowed silently) and the request still succeeds.
    if let Err(e) = nudge_user_push(&ctx.env, &uid, text).await {
        console_error!("expert_reply push nudge failed for user {uid}: {e}");
    }

    Response::from_json(&serde_json::json!({ "seq": result.seq }))
}

async fn expert_read(mut req: Request, ctx: RouteContext<()>) -> Result<Response> {
    if let Err(resp) = auth_expert(&req, &ctx.env).await {
        return Ok(resp);
    }
    let uid = ctx
        .param("uid")
        .ok_or_else(|| Error::RustError("missing uid".into()))?
        .clone();
    let body: serde_json::Value = req.json().await?;
    let seq = body
        .get("seq")
        .and_then(|v| v.as_i64())
        .ok_or_else(|| Error::RustError("missing seq".into()))?;
    let read_req = do_request("/read", &serde_json::json!({ "who": "expert", "seq": seq }))?;
    conversation_stub(&ctx.env, &uid)?
        .fetch_with_request(read_req)
        .await
}

// ---- ADMIN authorization handlers ----

/// GET /admin/me (user JWT). Returns the DO's {"approved":bool,"code":string|null}
/// verbatim so the admin UI knows whether to show the queue or request-access.
async fn admin_me(req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let sub = match auth_user(&req, &ctx.env).await {
        Ok(s) => s,
        Err(resp) => return Ok(resp),
    };
    let do_req = do_request("/admin-get", &serde_json::json!({ "sub": sub }))?;
    index_stub(&ctx.env)?.fetch_with_request(do_req).await
}

/// POST /admin/request (user JWT). The code maps to THIS token's authenticated
/// sub — never a body field (INVARIANT 3). Idempotent: returns the same code.
async fn admin_request(req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let sub = match auth_user(&req, &ctx.env).await {
        Ok(s) => s,
        Err(resp) => return Ok(resp),
    };
    let do_req = do_request("/admin-request", &serde_json::json!({ "sub": sub }))?;
    index_stub(&ctx.env)?.fetch_with_request(do_req).await
}

/// POST /admin/approve (X-Admin-Secret header, NO user JWT). Requires the header
/// to equal ADMIN_APPROVE_SECRET; an unset/empty secret fails closed (never
/// approve-anyone, INVARIANT 1). Only {code} is sent to the DO; the approved sub
/// is resolved from STORAGE there, never from this caller (INVARIANT 2).
async fn admin_approve(mut req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let secret = match admin_approve_secret(&ctx.env).await {
        Ok(s) => s,
        Err(e) => return Ok(json_status(500, &e)),
    };
    let provided = req
        .headers()
        .get("X-Admin-Secret")
        .ok()
        .flatten()
        .unwrap_or_default();
    if provided.is_empty() || provided != secret {
        return Ok(json_status(403, "bad admin secret"));
    }
    let body: serde_json::Value = req.json().await?;
    let code = body.get("code").and_then(|v| v.as_str()).unwrap_or("");
    if code.is_empty() {
        return Ok(json_status(400, "code is required"));
    }
    let do_req = do_request("/admin-approve", &serde_json::json!({ "code": code }))?;
    index_stub(&ctx.env)?.fetch_with_request(do_req).await
}

/// POST /internal/is-admin {sub} -> {approved}. Cross-worker admin check, called
/// by payment-worker's require_admin via the SUPPORT_WORKER service binding. SAME
/// source of truth as auth_expert: the DO `admins` table — one approved-admins
/// store, no redeploy to add an admin. Guarded by the
/// shared INTERNAL_PUSH_KEY (X-Internal-Key); an unset key fails closed (500), a
/// wrong/missing key 403s. NEVER swallows: any DO/stub/parse error 500s.
async fn internal_is_admin(mut req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let key = match token::secret_or_var(&ctx.env, "INTERNAL_PUSH_KEY").await {
        Ok(k) => k,
        Err(e) => return Ok(json_status(500, &e)),
    };
    let provided = req
        .headers()
        .get("X-Internal-Key")
        .ok()
        .flatten()
        .unwrap_or_default();
    if provided.is_empty() || provided != key {
        return Ok(json_status(403, "bad internal key"));
    }

    let body: serde_json::Value = req.json().await?;
    let sub = body.get("sub").and_then(|v| v.as_str()).unwrap_or("");
    if sub.is_empty() {
        return Ok(json_status(400, "sub required"));
    }

    // SAME logic as auth_expert: the DO admins table.
    let do_req = do_request("/admin-is-approved", &serde_json::json!({ "sub": sub }))?;
    let mut resp = index_stub(&ctx.env)?.fetch_with_request(do_req).await?;
    if resp.status_code() != 200 {
        return Ok(json_status(500, "admin auth DO error"));
    }
    let v: serde_json::Value = resp.json().await?;
    let approved = v.get("approved").and_then(|b| b.as_bool()).unwrap_or(false);
    Response::from_json(&serde_json::json!({ "approved": approved }))
}

/// Extract the typed-envelope fields from a message request body.
///
/// `kind` defaults to "text". `payload` is normalised to a RAW JSON STRING (the
/// storage/read contract): a JSON string passes through verbatim, a JSON object
/// is stringified, anything else (or absent) becomes None. This is forwarded to
/// the DO's `/append` and stored/returned unchanged.
fn typed_envelope(body: &serde_json::Value) -> (String, Option<String>) {
    let kind = body
        .get("kind")
        .and_then(|v| v.as_str())
        .unwrap_or("text")
        .to_string();
    let payload = match body.get("payload") {
        Some(serde_json::Value::String(s)) => Some(s.clone()),
        Some(serde_json::Value::Null) | None => None,
        Some(other) => Some(other.to_string()),
    };
    (kind, payload)
}

fn parse_paging(req: &Request) -> Result<(i64, i64)> {
    let url = req.url()?;
    let q: std::collections::HashMap<_, _> = url.query_pairs().into_owned().collect();
    let after_seq: i64 = q.get("after_seq").and_then(|s| s.parse().ok()).unwrap_or(0);
    let limit: i64 = q.get("limit").and_then(|s| s.parse().ok()).unwrap_or(50);
    Ok((after_seq, limit))
}

/// `?wait=<seconds>` → hold the /list open for up to that long (long-poll).
/// Absent/0 = immediate one-shot read. Clamped to [0, 25] s (the DO caps too).
fn parse_wait_ms(req: &Request) -> u64 {
    req.url()
        .ok()
        .and_then(|u| {
            u.query_pairs()
                .into_owned()
                .find(|(k, _)| k == "wait")
                .and_then(|(_, v)| v.parse::<u64>().ok())
        })
        .unwrap_or(0)
        .min(25)
        * 1000
}

// ---- CORS ----

fn is_allowed_origin(origin: &str) -> bool {
    origin == "https://renorma.app"
        || (origin.starts_with("https://") && origin.ends_with(".renorma.app"))
        || origin == "https://renorma-fit-dev.pages.dev"
        || origin == "https://renorma-admin-dev.pages.dev"
        || origin.starts_with("http://localhost")
        || origin.starts_with("http://127.0.0.1")
}

fn add_cors(resp: Response, origin: &str) -> Result<Response> {
    let headers = Headers::new();
    if is_allowed_origin(origin) {
        let _ = headers.set("Access-Control-Allow-Origin", origin);
    }
    let _ = headers.set("Vary", "Origin");
    let _ = headers.set("Access-Control-Allow-Methods", "GET, POST, OPTIONS");
    let _ = headers.set("Access-Control-Allow-Headers", "Content-Type, Authorization");
    for (k, v) in resp.headers() {
        let _ = headers.set(&k, &v);
    }
    let status = resp.status_code();
    Ok(Response::from_body(resp.body().clone())?
        .with_headers(headers)
        .with_status(status))
}

/// Resolve EVERY required Store-bound secret at the top of the fetch entry. On the
/// FIRST failure, log the full reason loudly and return a 503 so every request to a
/// misconfigured worker is obviously broken (and says why) instead of degrading into
/// a confusing 401/500 deeper in the request.
async fn require_secrets(env: &Env) -> std::result::Result<(), Response> {
    for name in ["JWT_SECRET", "INTERNAL_PUSH_KEY", "ADMIN_APPROVE_SECRET"] {
        if let Err(reason) = token::secret_or_var(env, name).await {
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

    if req.method() == Method::Options {
        let headers = Headers::new();
        if is_allowed_origin(&origin) {
            let _ = headers.set("Access-Control-Allow-Origin", &origin);
        }
        let _ = headers.set("Vary", "Origin");
        let _ = headers.set("Access-Control-Allow-Methods", "GET, POST, OPTIONS");
        let _ = headers.set("Access-Control-Allow-Headers", "Content-Type, Authorization");
        let _ = headers.set("Access-Control-Max-Age", "86400");
        return Ok(Response::empty()?.with_headers(headers).with_status(204));
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
        return Ok(resp);
    }

    let router = Router::new();

    let result = router
        // USER side (JWT sub = user_id; user owns its DO via idFromName(sub))
        .post_async("/message", user_send)
        .get_async("/messages", user_messages)
        .post_async("/read", user_read)
        // EXPERT side (JWT AND sub DO-approved in the admins table)
        .get_async("/conversations", expert_conversations)
        .get_async("/conversations/:uid/messages", expert_messages)
        .post_async("/conversations/:uid/reply", expert_reply)
        .post_async("/conversations/:uid/read", expert_read)
        // ADMIN authorization (request-code + operator secret; no redeploy to add)
        .get_async("/admin/me", admin_me)
        .post_async("/admin/request", admin_request)
        .post_async("/admin/approve", admin_approve)
        // INTERNAL: cross-worker admin check (payment-worker via service binding).
        .post_async("/internal/is-admin", internal_is_admin)
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

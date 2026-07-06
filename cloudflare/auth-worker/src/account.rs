// Provider-agnostic account/identity + code→token auth (ISOLATED from WebAuthn/phrase).
//
// Model: `user_id` is the anchor. An external identity (Telegram today) maps to it and is a
// delivery channel. Access = a session token for the user_id, created either by WebAuthn
// (existing flows) or by a one-time code delivered to the user's channel.

use worker::*;

use crate::token;
use crate::types::ErrorResponse;
use crate::{auth_do_stub, do_request};

fn err(status: u16, msg: &str) -> Result<Response> {
    Ok(Response::from_json(&ErrorResponse { error: msg.into() })?.with_status(status))
}

fn internal_key_ok(req: &Request, expected: &str) -> bool {
    let provided = req.headers().get("X-Internal-Key").ok().flatten().unwrap_or_default();
    !provided.is_empty() && provided == expected
}

async fn require_internal(req: &Request, ctx: &RouteContext<()>) -> std::result::Result<(), Response> {
    match token::secret_or_var(&ctx.env, "INTERNAL_PUSH_KEY").await {
        Ok(k) if !k.is_empty() && internal_key_ok(req, &k) => Ok(()),
        Ok(_) => Err(Response::from_json(&ErrorResponse { error: "unauthorized".into() })
            .unwrap()
            .with_status(403)),
        Err(e) => {
            console_error!("account: {e}");
            Err(Response::from_json(&ErrorResponse { error: "internal_not_configured".into() })
                .unwrap()
                .with_status(500))
        }
    }
}

fn gen_code() -> Result<String> {
    let mut b = [0u8; 4];
    getrandom::getrandom(&mut b).map_err(|e| Error::RustError(format!("getrandom: {e}")))?;
    Ok(format!("{:06}", u32::from_be_bytes(b) % 1_000_000))
}

fn hash_code(code: &str) -> String {
    use base64::Engine;
    use sha2::{Digest, Sha256};
    let mut h = Sha256::new();
    h.update(code.as_bytes());
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(h.finalize())
}

async fn do_call(ctx: &RouteContext<()>, path: &str, body: &serde_json::Value) -> Result<serde_json::Value> {
    let stub = auth_do_stub(&ctx.env)?;
    let req = do_request(path, body)?;
    let mut resp = stub.fetch_with_request(req).await?;
    if resp.status_code() != 200 {
        return Err(Error::RustError(format!("DO {path} → {}", resp.status_code())));
    }
    resp.json().await
}

// ── POST /internal/account-resolve {provider, providerUid, username?} ─────────────
/// INTERNAL. Resolve-or-create the account for an external identity (first touch). Called by
/// telegram-worker (/start, /miniapp/me) and payment-worker (checkout). Returns {userId}.
pub async fn account_resolve(mut req: Request, ctx: RouteContext<()>) -> Result<Response> {
    if let Err(resp) = require_internal(&req, &ctx).await {
        return Ok(resp);
    }
    let body: serde_json::Value = req.json().await.map_err(|e| Error::RustError(format!("body: {e}")))?;
    let provider = body.get("provider").and_then(|v| v.as_str()).unwrap_or("");
    let provider_uid = body.get("providerUid").and_then(|v| v.as_str()).unwrap_or("");
    if provider.is_empty() || provider_uid.is_empty() {
        return err(400, "missing provider/providerUid");
    }
    let username = body.get("username").and_then(|v| v.as_str());
    let mut do_body = serde_json::json!({ "provider": provider, "provider_uid": provider_uid });
    if let Some(u) = username {
        do_body["username"] = serde_json::Value::String(u.to_string());
    }
    let v = do_call(&ctx, "/account/resolve", &do_body).await?;
    Response::from_json(&v)
}

// ── POST /internal/code/mint {userId} ─────────────────────────────────────────────
/// INTERNAL. Mint a one-time code and RETURN it (no delivery, no cooldown) — for the trusted
/// Mini App, which validated the Telegram identity itself and embeds the code in the onboard
/// link so iOS/desktop can auto-authorize without asking the user to copy it. Returns {code}.
pub async fn code_mint(mut req: Request, ctx: RouteContext<()>) -> Result<Response> {
    if let Err(resp) = require_internal(&req, &ctx).await {
        return Ok(resp);
    }
    let body: serde_json::Value = req.json().await.map_err(|e| Error::RustError(format!("body: {e}")))?;
    let user_id = body.get("userId").and_then(|v| v.as_str()).unwrap_or("");
    if user_id.is_empty() {
        return err(400, "missing userId");
    }
    let code = gen_code()?;
    let code_hash = hash_code(&code);
    do_call(&ctx, "/code/mint", &serde_json::json!({ "user_id": user_id, "code_hash": code_hash })).await?;
    Response::from_json(&serde_json::json!({ "code": code }))
}

// ── POST /internal/has-credentials {userId} ───────────────────────────────────────
/// INTERNAL. Does this account have any passkey? Used by the admin «paid but no credentials»
/// worklist (a paid user with zero passkeys hasn't set up durable access).
pub async fn has_credentials(mut req: Request, ctx: RouteContext<()>) -> Result<Response> {
    if let Err(resp) = require_internal(&req, &ctx).await {
        return Ok(resp);
    }
    let body: serde_json::Value = req.json().await.map_err(|e| Error::RustError(format!("body: {e}")))?;
    let user_id = body.get("userId").and_then(|v| v.as_str()).unwrap_or("");
    if user_id.is_empty() {
        return err(400, "missing userId");
    }
    let v = do_call(&ctx, "/credentials/count", &serde_json::json!({ "user_id": user_id })).await?;
    let count = v.get("count").and_then(|x| x.as_i64()).unwrap_or(0);
    Response::from_json(&serde_json::json!({ "hasCredentials": count > 0 }))
}

// ── POST /code/request {userId} (PUBLIC; user_id is non-secret) ───────────────────
/// Generate a one-time code, arm the per-user cooldown, and deliver it to the user's channel
/// (Telegram → our payment bot). Within the cooldown → 429 {waitMs}. The code itself is the
/// bearer of a session for THIS user, so it only ever goes to the user's own channel.
pub async fn code_request(mut req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let body: serde_json::Value = req.json().await.unwrap_or(serde_json::json!({}));
    let user_id = body.get("userId").and_then(|v| v.as_str()).unwrap_or("");
    if user_id.is_empty() {
        return err(400, "missing userId");
    }
    let code = gen_code()?;
    let code_hash = hash_code(&code);

    // Issue (cooldown-gated) BEFORE sending, so a spammer can't blast messages.
    let issued = do_call(&ctx, "/code/issue", &serde_json::json!({ "user_id": user_id, "code_hash": code_hash })).await?;
    if issued.get("ok").and_then(|v| v.as_bool()) != Some(true) {
        let wait = issued.get("waitMs").and_then(|v| v.as_i64()).unwrap_or(0);
        return Ok(Response::from_json(&serde_json::json!({ "error": "cooldown", "waitMs": wait }))?.with_status(429));
    }

    // Resolve the delivery channel + deliver.
    let idv = do_call(&ctx, "/identity", &serde_json::json!({ "user_id": user_id })).await?;
    let identity = idv.get("identity");
    let provider = identity.and_then(|i| i.get("provider")).and_then(|v| v.as_str()).unwrap_or("");
    let provider_uid = identity.and_then(|i| i.get("provider_uid")).and_then(|v| v.as_str()).unwrap_or("");
    if provider.is_empty() || provider_uid.is_empty() {
        return err(409, "no_channel");
    }
    if let Err(e) = deliver_code(&ctx, provider, provider_uid, &code).await {
        console_error!("code_request: deliver failed: {e}");
        return err(502, "deliver_failed");
    }
    Response::from_json(&serde_json::json!({ "ok": true }))
}

// ── POST /code/verify {userId, code} (PUBLIC) ─────────────────────────────────────
/// Verify + consume the code → mint a session token for the user_id. This is the code login.
pub async fn code_verify(mut req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let body: serde_json::Value = req.json().await.unwrap_or(serde_json::json!({}));
    let user_id = body.get("userId").and_then(|v| v.as_str()).unwrap_or("");
    let code = body.get("code").and_then(|v| v.as_str()).map(|s| s.trim()).unwrap_or("");
    if user_id.is_empty() || code.is_empty() {
        return err(400, "missing userId/code");
    }
    let code_hash = hash_code(code);
    let consumed = do_call(&ctx, "/code/consume", &serde_json::json!({ "code_hash": code_hash })).await?;
    if consumed.get("ok").and_then(|v| v.as_bool()) != Some(true) {
        return err(401, "invalid_code");
    }
    // The code hash maps to its true owner; cross-check it matches the claimed user_id.
    let owner = consumed.get("userId").and_then(|v| v.as_str()).unwrap_or("");
    if owner != user_id {
        return err(401, "invalid_code");
    }
    // Mint the session (same generic primitives as every login path).
    let secret = token::jwt_secret(&ctx.env).await?;
    let (token_response, token_id) = token::create_token(user_id, "", vec!["auth".to_string()], &secret)?;
    token::store_token_in_do(&ctx.env, &token_id, user_id, "").await?;
    Response::from_json(&serde_json::json!({ "userId": user_id, "token": token_response.token }))
}

// ── POST /chapters/available {chapter} (app JWT) ──────────────────────────────────
/// Record — from the RUNNING app — that a story chapter became available in the UI for this
/// user. Stored next to the persona. The FIRST chapter unlocking = «entered the system». The
/// session token identifies the user (no user_id in the body).
pub async fn chapter_available(mut req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let user_id = match token::validate_from_header(&req, &ctx.env).await {
        Ok(uid) => uid,
        Err(e) => return err(401, &format!("authentication required: {e}")),
    };
    let body: serde_json::Value = req.json().await.unwrap_or(serde_json::json!({}));
    let chapter = body
        .get("chapter")
        .and_then(|v| v.as_str())
        .map(|s| s.trim())
        .unwrap_or("");
    if chapter.is_empty() {
        return err(400, "missing chapter");
    }
    let v = do_call(
        &ctx,
        "/chapters/available",
        &serde_json::json!({ "user_id": user_id, "chapter": chapter }),
    )
    .await?;
    Response::from_json(&v)
}

// ── POST /internal/has-entered {userId} ───────────────────────────────────────────
/// INTERNAL. Has the user reached the app (the first story chapter became available)? The Mini
/// App uses this to decide «Открыть приложение» vs «Получить доступ» — «access obtained» is
/// «entered the app», independent of whether the user set up a passkey.
pub async fn has_entered(mut req: Request, ctx: RouteContext<()>) -> Result<Response> {
    if let Err(resp) = require_internal(&req, &ctx).await {
        return Ok(resp);
    }
    let body: serde_json::Value = req.json().await.map_err(|e| Error::RustError(format!("body: {e}")))?;
    let user_id = body.get("userId").and_then(|v| v.as_str()).unwrap_or("");
    if user_id.is_empty() {
        return err(400, "missing userId");
    }
    let v = do_call(&ctx, "/has-entered", &serde_json::json!({ "user_id": user_id })).await?;
    Response::from_json(&v)
}

/// Deliver a code over the user's channel. Provider-agnostic switch point (Telegram today).
async fn deliver_code(ctx: &RouteContext<()>, provider: &str, provider_uid: &str, code: &str) -> Result<()> {
    match provider {
        "telegram" => {
            let tg_id: i64 = provider_uid
                .parse()
                .map_err(|_| Error::RustError("bad telegram uid".into()))?;
            let key = token::secret_or_var(&ctx.env, "INTERNAL_PUSH_KEY")
                .await
                .map_err(Error::RustError)?;
            let text = format!("Код: {code}\n\nВы запросили код для входа в приложение.");
            let payload = serde_json::json!({ "tgUserId": tg_id, "text": text }).to_string();
            let headers = Headers::new();
            headers.set("Content-Type", "application/json")?;
            headers.set("X-Internal-Key", &key)?;
            let mut init = RequestInit::new();
            init.with_method(Method::Post)
                .with_headers(headers)
                .with_body(Some(wasm_bindgen::JsValue::from_str(&payload)));
            let request = Request::new_with_init("https://telegram-worker/internal/send", &init)?;
            let tg = ctx
                .env
                .service("TELEGRAM_WORKER")
                .map_err(|e| Error::RustError(format!("TELEGRAM_WORKER binding: {e}")))?;
            let mut res = tg.fetch_request(request).await?;
            if !(200..300).contains(&res.status_code()) {
                let t = res.text().await.unwrap_or_default();
                return Err(Error::RustError(format!("send {}: {t}", res.status_code())));
            }
            Ok(())
        }
        other => Err(Error::RustError(format!("unsupported provider: {other}"))),
    }
}

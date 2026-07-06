// Telegram-pay bot worker.
//
// Flow: the bot is a thin front door — any message (/start) replies with a button that opens
// the Mini App (tg.renorma.app), where the promo is entered and the payment happens. The
// Mini App calls payment-worker /internal/checkout (INTERNAL_PUSH_KEY-guarded) to mint a lava
// invoice; payment-worker writes the claim → {tg_id, secret} binding into its own ClaimDO.
// After the SIGNED lava webhook marks the claim paid, payment-worker calls our /internal/paid
// {claimId} and we send the user a button that reopens the Mini App to collect onboarding.
//
// MONEY-SAFETY: `paid` is set ONLY by the signed lava webhook (in payment-worker); this bot
// never sets it. The claim secret lives ONLY in payment-worker's ClaimDO and is NEVER logged.
// Inbound webhook is verified by the Telegram secret-token header; the internal route is
// verified by INTERNAL_PUSH_KEY. Misconfigured secrets fail loud.

use wasm_bindgen::JsValue;
use worker::*;

mod init_data;
mod miniapp;
pub(crate) mod token;
mod types;

use types::Update;

// ── Telegram Bot API ─────────────────────────────────────────────────────────
/// POST to https://api.telegram.org/bot<token>/<method>. The bot token is ONLY ever
/// a URL path segment and is NEVER logged. On non-2xx the status (+ body) is logged
/// loudly, but never the request URL/token.
async fn tg_api(env: &Env, method: &str, body: &serde_json::Value) -> Result<()> {
    let token = token::secret_or_var(env, "TELEGRAM_BOT_TOKEN")
        .await
        .map_err(Error::RustError)?;
    let url = format!("https://api.telegram.org/bot{token}/{method}");
    let body_str = serde_json::to_string(body)
        .map_err(|e| Error::RustError(format!("serialize tg body: {e}")))?;
    let headers = Headers::new();
    headers
        .set("Content-Type", "application/json")
        .map_err(|e| Error::RustError(format!("set header: {e}")))?;
    let mut init = RequestInit::new();
    init.with_method(Method::Post)
        .with_headers(headers)
        .with_body(Some(JsValue::from_str(&body_str)));
    let req = Request::new_with_init(&url, &init)?;
    match Fetch::Request(req).send().await {
        Ok(mut res) => {
            let status = res.status_code();
            if !(200..300).contains(&status) {
                let txt = res.text().await.unwrap_or_default();
                console_error!("tg_api {method}: {status} {txt}");
            }
            Ok(())
        }
        Err(e) => {
            console_error!("tg_api {method} fetch failed: {e}");
            Err(e)
        }
    }
}

/// sendMessage with an optional inline keyboard (reply_markup).
async fn send_message(
    env: &Env,
    chat_id: i64,
    text: &str,
    reply_markup: Option<serde_json::Value>,
) -> Result<()> {
    let mut body = serde_json::json!({ "chat_id": chat_id, "text": text });
    if let Some(markup) = reply_markup {
        body["reply_markup"] = markup;
    }
    tg_api(env, "sendMessage", &body).await
}

/// POST /internal/send {tgUserId, text} — INTERNAL_PUSH_KEY-gated. Sends a plain bot message
/// to a Telegram user (chat_id == user id in a private chat). Used by payment-worker to
/// deliver the no-passkey login code. Fails closed on a bad/missing key.
async fn internal_send(mut req: Request, env: &Env) -> Result<Response> {
    let key = match token::secret_or_var(env, "INTERNAL_PUSH_KEY").await {
        Ok(k) => k,
        Err(e) => {
            console_error!("internal_send: {e}");
            return Ok(error_response("internal_not_configured", 500));
        }
    };
    let provided = req
        .headers()
        .get("X-Internal-Key")
        .ok()
        .flatten()
        .unwrap_or_default();
    if provided.is_empty() || provided != key {
        return Ok(error_response("bad internal key", 403));
    }
    let body: serde_json::Value = req.json().await.unwrap_or(serde_json::json!({}));
    let tg_user_id = match body.get("tgUserId").and_then(|v| v.as_i64()) {
        Some(id) => id,
        None => return Ok(error_response("missing tgUserId", 400)),
    };
    let text = body.get("text").and_then(|v| v.as_str()).unwrap_or("");
    if text.is_empty() {
        return Ok(error_response("missing text", 400));
    }
    send_message(env, tg_user_id, text, None).await?;
    Response::from_json(&serde_json::json!({ "ok": true }))
}


// ── secrets ─────────────────────────────────────────────────────────────────
/// Resolve every REQUIRED secret at the top of the fetch entry. First failure → log
/// the full reason loudly and 503 (Workers have no separate startup — per-request is
/// intended). Mirrors payment-worker `require_secrets`.
async fn require_secrets(env: &Env) -> std::result::Result<(), Response> {
    for name in ["TELEGRAM_BOT_TOKEN", "TELEGRAM_WEBHOOK_SECRET", "INTERNAL_PUSH_KEY"] {
        if let Err(reason) = token::secret_or_var(env, name).await {
            console_error!("STARTUP MISCONFIG: {name}: {reason}");
            let body = format!("MISCONFIGURED: {name} — {reason}");
            return Err(Response::error(body, 503)
                .unwrap_or_else(|_| Response::error("MISCONFIGURED", 503).unwrap()));
        }
    }
    Ok(())
}

pub(crate) fn error_response(message: &str, status: u16) -> Response {
    Response::from_json(&serde_json::json!({ "error": message }))
        .expect("serialize error")
        .with_status(status)
}

#[event(fetch)]
async fn main(req: Request, env: Env, _ctx: Context) -> Result<Response> {
    if let Err(resp) = require_secrets(&env).await {
        return Ok(resp);
    }
    match handle(req, &env).await {
        Ok(r) => Ok(r),
        Err(e) => Ok(error_response(&e.to_string(), 500)),
    }
}

async fn handle(req: Request, env: &Env) -> Result<Response> {
    let url = req.url()?;
    let path = url.path().to_string();
    let method = req.method();

    if method == Method::Post && path == "/webhook/telegram" {
        return webhook_telegram(req, env).await;
    }
    if method == Method::Post && path == "/internal/paid" {
        return internal_paid(req, env).await;
    }
    if method == Method::Post && path == "/internal/cancelled" {
        return internal_cancelled(req, env).await;
    }
    if method == Method::Post && path == "/internal/send" {
        return internal_send(req, env).await;
    }
    // ── Telegram Mini App pay flow (page + same-origin APIs) ──
    if method == Method::Get && path == "/" {
        return miniapp::serve_miniapp_page(env);
    }
    if method == Method::Post && path == "/miniapp/price" {
        return miniapp::miniapp_price(req, env).await;
    }
    if method == Method::Post && path == "/miniapp/checkout" {
        return miniapp::miniapp_checkout(req, env).await;
    }
    if method == Method::Post && path == "/miniapp/status" {
        return miniapp::miniapp_status(req, env).await;
    }
    if method == Method::Post && path == "/miniapp/me" {
        return miniapp::miniapp_me(req, env).await;
    }
    if method == Method::Post && path == "/miniapp/access-link" {
        return miniapp::miniapp_access_link(req, env).await;
    }
    Ok(error_response("Not found", 404))
}

// ── POST /webhook/telegram ─────────────────────────────────────────────────────
async fn webhook_telegram(mut req: Request, env: &Env) -> Result<Response> {
    // [SECURITY CHECKPOINT #1] Telegram secret-token gate (fail loud).
    let secret = match token::secret_or_var(env, "TELEGRAM_WEBHOOK_SECRET").await {
        Ok(s) => s,
        Err(e) => {
            console_error!("webhook_telegram: {e}");
            return Ok(error_response("misconfigured", 503));
        }
    };
    let provided = req
        .headers()
        .get("X-Telegram-Bot-Api-Secret-Token")
        .ok()
        .flatten()
        .unwrap_or_default();
    if provided.is_empty() || provided != secret {
        return Ok(error_response("unauthorized", 401));
    }

    let update: Update = match req.json().await {
        Ok(u) => u,
        Err(e) => {
            console_error!("webhook_telegram: parse update failed: {e}");
            return Ok(error_response("bad_request", 400));
        }
    };

    // The bot is a thin front door: any message (/start or otherwise) just opens the Mini App,
    // where the promo is entered and the payment happens. No inline-button / callback flow.
    if let Some(msg) = &update.message {
        let chat_id = msg.chat.id;
        let text = msg.text.as_deref().unwrap_or("").trim();
        if text == "/start" || !text.is_empty() {
            send_welcome(env, chat_id).await?;
        }
        return Ok(ok_200());
    }

    // Unrecognized update → 200 no-op (Telegram needs a 2xx).
    Ok(ok_200())
}

/// The Mini App URL (env-driven `MINIAPP_URL`: dev = the dev telegram-worker host, prod =
/// tg.renorma.app). Not hardcoded so the dev bot opens the dev Mini App.
fn miniapp_url(env: &Env) -> String {
    env.var("MINIAPP_URL").map(|v| v.to_string()).unwrap_or_default()
}

/// Welcome shown on /start (and any message / the "start" button): open the Mini App,
/// where the promo code is entered and the payment happens.
async fn send_welcome(env: &Env, chat_id: i64) -> Result<()> {
    let url = miniapp_url(env);
    send_message(
        env,
        chat_id,
        "Откройте мини-приложение, чтобы оформить подписку — там вводится промокод и проходит оплата.",
        Some(miniapp::inline_keyboard_web_app("Открыть приложение", &url)),
    )
    .await
}

pub(crate) struct CheckoutResult {
    pub(crate) pay_url: String,
    pub(crate) claim_id: String,
    /// lava-decoded price (paymentParams.amount_total, promo-applied). Optional — a
    /// missing amount is NOT an error; the Mini App shows '…' instead of a price.
    pub(crate) amount: Option<f64>,
    pub(crate) currency: Option<String>,
    /// Invoice lifetime in ms (server INVOICE_TTL_MS), for the client expiry watch.
    pub(crate) ttl_ms: Option<i64>,
}

/// POST payment-worker /internal/checkout {promoCode, tg…} with X-Internal-Key. The
/// offer to sell is decided by payment-worker (LAVA_OFFER_ID) — no planId sent.
/// Expects {payUrl, claimId}. The claim→{tg_id, secret} binding is written by
/// payment-worker itself into ClaimDO (tg_claims), so we never handle the secret here.
/// Any non-2xx / parse / binding error → Err (the caller logs + surfaces; never invents success).
pub(crate) async fn call_internal_checkout(
    env: &Env,
    promo_code: Option<&str>,
    tg_user_id: i64,
    tg_username: Option<&str>,
    currency: &str,
    payment_method: &str,
) -> Result<CheckoutResult> {
    let key = token::secret_or_var(env, "INTERNAL_PUSH_KEY")
        .await
        .map_err(Error::RustError)?;
    // currency + paymentMethod are REQUIRED and always sent explicitly; promo is optional
    // (absent → no promo, full price).
    let mut body = serde_json::json!({
        "tgUserId": tg_user_id,
        "currency": currency,
        "paymentMethod": payment_method,
    });
    if let Some(p) = promo_code {
        body["promoCode"] = serde_json::Value::String(p.to_string());
    }
    if let Some(u) = tg_username {
        body["tgUsername"] = serde_json::Value::String(u.to_string());
    }
    let body_str = serde_json::to_string(&body)?;
    let headers = Headers::new();
    let _ = headers.set("Content-Type", "application/json");
    let _ = headers.set("X-Internal-Key", &key);
    let mut init = RequestInit::new();
    init.with_method(Method::Post)
        .with_headers(headers)
        .with_body(Some(JsValue::from_str(&body_str)));
    // Host is irrelevant for a service-binding fetch; only the path routes.
    let request = Request::new_with_init("https://payment-worker/internal/checkout", &init)?;
    let payment = env
        .service("PAYMENT_WORKER")
        .map_err(|e| Error::RustError(format!("PAYMENT_WORKER binding: {e}")))?;
    let mut resp = payment.fetch_request(request).await?;
    let status = resp.status_code();
    if !(200..300).contains(&status) {
        let txt = resp.text().await.unwrap_or_default();
        return Err(Error::RustError(format!("checkout status {status}: {txt}")));
    }
    let v: serde_json::Value = resp.json().await?;
    let pay_url = v
        .get("payUrl")
        .and_then(|x| x.as_str())
        .ok_or_else(|| Error::RustError("checkout response missing payUrl".into()))?
        .to_string();
    let claim_id = v
        .get("claimId")
        .and_then(|x| x.as_str())
        .ok_or_else(|| Error::RustError("checkout response missing claimId".into()))?
        .to_string();
    // Optional lava-decoded price — a missing amount is not an error (client shows '…').
    let amount = v.get("amount").and_then(|x| x.as_f64());
    let currency = v.get("currency").and_then(|x| x.as_str()).map(String::from);
    // Invoice lifetime (ms), relayed to the client's expiry watch. Optional: a missing value
    // just makes the watch inert (it's a UX guard, not money), never blocks checkout.
    let ttl_ms = v.get("ttlMs").and_then(|x| x.as_i64());
    Ok(CheckoutResult {
        pay_url,
        claim_id,
        amount,
        currency,
        ttl_ms,
    })
}

// ── POST /internal/paid (INTERNAL_PUSH_KEY-guarded, idempotent) ─────────────────
async fn internal_paid(mut req: Request, env: &Env) -> Result<Response> {
    // [SECURITY CHECKPOINT #3] internal-key gate (fail closed).
    let key = match token::secret_or_var(env, "INTERNAL_PUSH_KEY").await {
        Ok(k) => k,
        Err(e) => {
            console_error!("internal_paid: {e}");
            return Ok(error_response("internal_not_configured", 500));
        }
    };
    let provided = req
        .headers()
        .get("X-Internal-Key")
        .ok()
        .flatten()
        .unwrap_or_default();
    if provided.is_empty() || provided != key {
        return Ok(error_response("bad internal key", 403));
    }

    let body: serde_json::Value = req.json().await.unwrap_or(serde_json::json!({}));
    let claim_id = body.get("claimId").and_then(|v| v.as_str()).unwrap_or("");
    if claim_id.is_empty() {
        return Ok(error_response("missing claimId", 400));
    }

    // The Telegram binding + secret now live in payment-worker's ClaimDO (tg_claims) —
    // ONE store, no chat-vs-miniapp split. Look it up there.
    let cv = miniapp::payment_tg(env, "get", serde_json::json!({ "claimId": claim_id })).await?;
    if cv.get("found").and_then(|v| v.as_bool()) != Some(true) {
        return Response::from_json(&serde_json::json!({ "ok": true, "skipped": "unknown_claim" }));
    }
    // Idempotent: already delivered → no-op 200.
    if cv.get("notifiedAt").and_then(|v| v.as_i64()).is_some() {
        return Response::from_json(&serde_json::json!({ "ok": true, "skipped": "already_notified" }));
    }
    let tg_id = match cv.get("tgId").and_then(|v| v.as_i64()) {
        Some(c) => c,
        None => return Ok(error_response("claim missing tgId", 500)),
    };

    // web_app button that REOPENS the Mini App (NOT a raw onboard URL): a URL button opens
    // Telegram's in-app browser, where passkey/WebAuthn fails — the Mini App's openLink →
    // Safari is the working path. The Mini App re-derives the onboard link from /miniapp/me.
    send_message(
        env,
        tg_id,
        "Оплата прошла успешно! Откройте приложение, чтобы получить доступ к re:Norma.",
        Some(miniapp::inline_keyboard_web_app("Открыть re:Norma", &miniapp_url(env))),
    )
    .await?;

    // Record delivery (idempotent).
    miniapp::payment_tg(env, "mark-notified", serde_json::json!({ "claimId": claim_id })).await?;

    Response::from_json(&serde_json::json!({ "ok": true }))
}

fn ok_200() -> Response {
    Response::from_json(&serde_json::json!({ "ok": true })).expect("serialize ok")
}

/// Russian day-count declension: 1 день, 2 дня, 5 дней.
pub(crate) fn ru_days(n: i64) -> &'static str {
    let n100 = n % 100;
    let n10 = n % 10;
    if (11..=14).contains(&n100) {
        "дней"
    } else if n10 == 1 {
        "день"
    } else if (2..=4).contains(&n10) {
        "дня"
    } else {
        "дней"
    }
}

// ── POST /internal/cancelled (INTERNAL_PUSH_KEY-guarded) ────────────────────────
/// payment-worker calls this after the user cancels in the app. We echo it to the bot:
/// "subscription cancelled — access for N more days". Best-effort (Mini App still shows
/// the live status on open); a private chat's chat_id == user.id.
async fn internal_cancelled(mut req: Request, env: &Env) -> Result<Response> {
    let key = match token::secret_or_var(env, "INTERNAL_PUSH_KEY").await {
        Ok(k) => k,
        Err(e) => {
            console_error!("internal_cancelled: {e}");
            return Ok(error_response("internal_not_configured", 500));
        }
    };
    let provided = req
        .headers()
        .get("X-Internal-Key")
        .ok()
        .flatten()
        .unwrap_or_default();
    if provided.is_empty() || provided != key {
        return Ok(error_response("bad internal key", 403));
    }

    let body: serde_json::Value = req.json().await.unwrap_or(serde_json::json!({}));
    let tg_user_id = match body.get("tgUserId").and_then(|v| v.as_i64()) {
        Some(id) => id,
        None => return Ok(error_response("missing tgUserId", 400)),
    };
    let days_left = body.get("daysLeft").and_then(|v| v.as_i64()).unwrap_or(0).max(0);

    let text = format!(
        "Подписка отменена. Доступ к re:Norma сохранится ещё {} {}.",
        days_left,
        ru_days(days_left)
    );
    send_message(env, tg_user_id, &text, None).await?;
    Ok(ok_200())
}

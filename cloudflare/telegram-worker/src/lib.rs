// Telegram-pay bot worker.
//
// Flow: /start (or the "НАЧАТЬ ПОХУДЕНИЕ" button) → promo prompt. The user may type
// a promo code (last typed wins, stored per chat in TgSessionDO). Pressing [Оплатить]
// creates a guest checkout via payment-worker /internal/checkout (INTERNAL_PUSH_KEY-
// guarded; carries the entered promoCode), stores the returned claim secret, and
// sends the user the lava payUrl. After the SIGNED lava webhook marks the claim paid,
// payment-worker calls our /internal/paid {claimId} and we send the user a claim-
// binding link https://fit.renorma.app/onboard#claim=<claimId>.<secret>.
//
// MONEY-SAFETY: `paid` is set ONLY by the signed lava webhook (in payment-worker);
// this bot never sets it. The claim secret lives ONLY in TgSessionDO and is NEVER
// logged. Inbound webhook is verified by the Telegram secret-token header; the
// internal route is verified by INTERNAL_PUSH_KEY. Misconfigured secrets fail loud.

use wasm_bindgen::JsValue;
use worker::*;

mod init_data;
mod miniapp;
mod tg_session_do;
pub(crate) mod token;
mod types;

pub use tg_session_do::TgSessionDO;

use types::Update;

// ── DO-stub helper ─────────────────────────────────────────────────────────────
// Storage epoch: BUMP to wipe TgSessionDO state (miniapp_claims etc.) in one deploy —
// the worker addresses a fresh, empty instance; the old one orphans. Avoids
// delete-class migrations (rejected while the binding references the class).
const DO_EPOCH: &str = "v2";

pub(crate) fn session_stub(env: &Env) -> Result<worker::durable::Stub> {
    env.durable_object("TG_SESSION_DO")?
        .id_from_name(&format!("global-{DO_EPOCH}"))?
        .get_stub()
}

/// POST to a DO stub at `https://do{path}` with a JSON body. Returns the raw Response.
pub(crate) async fn do_post(
    stub: &worker::durable::Stub,
    path: &str,
    body: &serde_json::Value,
) -> Result<Response> {
    let url = format!("https://do{path}");
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
    let req = Request::new_with_init(&url, &init)?;
    stub.fetch_with_request(req).await
}

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

async fn answer_callback_query(env: &Env, callback_query_id: &str) -> Result<()> {
    let body = serde_json::json!({ "callback_query_id": callback_query_id });
    tg_api(env, "answerCallbackQuery", &body).await
}

fn inline_keyboard_callback(text: &str, callback_data: &str) -> serde_json::Value {
    serde_json::json!({
        "inline_keyboard": [[ { "text": text, "callback_data": callback_data } ]]
    })
}

fn inline_keyboard_url(text: &str, url: &str) -> serde_json::Value {
    serde_json::json!({
        "inline_keyboard": [[ { "text": text, "url": url } ]]
    })
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
    // ── Telegram Mini App pay flow (page + same-origin APIs) ──
    if method == Method::Get && path == "/" {
        return miniapp::serve_miniapp_page();
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

    // callback_query takes precedence (a tapped button).
    if let Some(cq) = &update.callback_query {
        let chat_id = cq.message.as_ref().map(|m| m.chat.id);
        let data = cq.data.as_deref().unwrap_or("");
        // answerCallbackQuery is best-effort (logged on failure).
        let _ = answer_callback_query(env, &cq.id).await;
        let chat_id = match chat_id {
            Some(c) => c,
            None => return Ok(ok_200()),
        };
        match data {
            "start" => {
                send_welcome(env, chat_id).await?;
            }
            "pay" => {
                handle_pay(env, chat_id).await?;
            }
            _ => {}
        }
        return Ok(ok_200());
    }

    if let Some(msg) = &update.message {
        let chat_id = msg.chat.id;
        let text = msg.text.as_deref().unwrap_or("").trim();
        if text == "/start" || !text.is_empty() {
            // Promo + payment now live in the Mini App; the bot just opens it.
            send_welcome(env, chat_id).await?;
        }
        return Ok(ok_200());
    }

    // Unrecognized update → 200 no-op (Telegram needs a 2xx).
    Ok(ok_200())
}

/// Welcome shown on /start (and any message / the "start" button): open the Mini App,
/// where the promo code is entered and the payment happens.
async fn send_welcome(env: &Env, chat_id: i64) -> Result<()> {
    send_message(
        env,
        chat_id,
        "Откройте мини-приложение, чтобы оформить подписку — там вводится промокод и проходит оплата.",
        Some(miniapp::inline_keyboard_web_app(
            "Открыть приложение",
            "https://tg.renorma.app/",
        )),
    )
    .await
}

/// [Оплатить] → create the guest checkout (carrying the stored promo) and send payUrl.
async fn handle_pay(env: &Env, chat_id: i64) -> Result<()> {
    let stub = session_stub(env)?;

    // Read stored promo (may be null).
    let mut promo_res = do_post(
        &stub,
        "/session/get-promo",
        &serde_json::json!({ "chatId": chat_id }),
    )
    .await?;
    let promo_json: serde_json::Value = promo_res.json().await.unwrap_or(serde_json::json!({}));
    let promo_code = promo_json
        .get("promoCode")
        .and_then(|v| v.as_str())
        .map(String::from);

    // Call payment-worker /internal/checkout over the service binding. In a private chat
    // chat_id == user.id; the bot API has no @username here, so pass None.
    let checkout = match call_internal_checkout(env, promo_code.as_deref(), chat_id, None).await {
        Ok(c) => c,
        Err(e) => {
            // Already has an active/paid subscription (F-2) → not an error; tell them.
            if e.to_string().contains("ALREADY_ACTIVE") {
                send_message(
                    env,
                    chat_id,
                    "У вас уже есть доступ к re:Norma. Откройте приложение из меню бота.",
                    None,
                )
                .await?;
                return Ok(());
            }
            // No silent swallow: log loudly AND surface to the user. Return Ok so
            // Telegram doesn't retry-storm; NO claim stored.
            console_error!("handle_pay: internal/checkout failed: {e}");
            send_message(
                env,
                chat_id,
                "Не удалось создать оплату. Попробуйте позже.",
                None,
            )
            .await?;
            return Ok(());
        }
    };

    // [SECURITY CHECKPOINT #2] Store claimId → {chat_id, secret}. The plaintext secret
    // lives ONLY here and is NEVER logged. On a REUSED invoice (F-2) the secret was already
    // stored the first time and comes back empty — skip the put so we don't clobber it.
    if !checkout.reused {
        do_post(
            &stub,
            "/claims/put",
            &serde_json::json!({
                "claimId": checkout.claim_id,
                "chatId": chat_id,
                "secret": checkout.secret,
            }),
        )
        .await?;
    }

    send_message(
        env,
        chat_id,
        "Ссылка для оплаты подписки.",
        Some(inline_keyboard_url("Оплатить", &checkout.pay_url)),
    )
    .await
}

pub(crate) struct CheckoutResult {
    pub(crate) pay_url: String,
    pub(crate) claim_id: String,
    pub(crate) secret: String,
    /// True when payment-worker reused an existing pending invoice for this user (F-2).
    /// On reuse `secret` is empty — the caller must NOT re-store (would clobber the
    /// secret it saved the first time).
    pub(crate) reused: bool,
}

/// POST payment-worker /internal/checkout {promoCode, tg…} with X-Internal-Key. The
/// offer to sell is decided by payment-worker (LAVA_OFFER_ID) — no planId sent.
/// Expects {payUrl, claimId, secret}. Any non-2xx / parse / binding error → Err
/// (the caller logs + surfaces to the user; never invents success).
pub(crate) async fn call_internal_checkout(
    env: &Env,
    promo_code: Option<&str>,
    tg_user_id: i64,
    tg_username: Option<&str>,
) -> Result<CheckoutResult> {
    let key = token::secret_or_var(env, "INTERNAL_PUSH_KEY")
        .await
        .map_err(Error::RustError)?;
    let mut body = serde_json::json!({ "tgUserId": tg_user_id });
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
    let reused = v.get("reused").and_then(|x| x.as_bool()).unwrap_or(false);
    // On a reused pending invoice the secret is intentionally absent (already stored the
    // first time) — empty is expected, not an error.
    let secret = v
        .get("secret")
        .and_then(|x| x.as_str())
        .unwrap_or_default()
        .to_string();
    if !reused && secret.is_empty() {
        return Err(Error::RustError("checkout response missing secret".into()));
    }
    Ok(CheckoutResult { pay_url, claim_id, secret, reused })
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

    let stub = session_stub(env)?;
    let mut got = do_post(&stub, "/claims/get", &serde_json::json!({ "claimId": claim_id })).await?;
    let cv: serde_json::Value = got.json().await?;
    if cv.get("found").and_then(|v| v.as_bool()) == Some(false) {
        // Not a chat-flow claim. Try the Mini App claim (keyed by tg_user_id): notify the
        // user in the bot that payment went through, with a button reopening the Mini App.
        return internal_paid_miniapp(env, &stub, claim_id).await;
    }
    // Already delivered → no-op 200. Prevents a second onboard-link message if
    // payment-worker calls /internal/paid more than once for the same claim
    // (e.g. a distinct recurring event on the same contract, or a manual replay).
    if cv.get("notifiedAt").and_then(|v| v.as_i64()).is_some() {
        return Response::from_json(&serde_json::json!({ "ok": true, "skipped": "already_notified" }));
    }
    let chat_id = match cv.get("chatId").and_then(|v| v.as_i64()) {
        Some(c) => c,
        None => return Ok(error_response("claim missing chatId", 500)),
    };
    let secret = match cv.get("secret").and_then(|v| v.as_str()) {
        Some(s) => s.to_string(),
        None => return Ok(error_response("claim missing secret", 500)),
    };

    let base = env
        .var("ONBOARD_BASE_URL")
        .map(|v| v.to_string())
        .unwrap_or_else(|_| "https://fit.renorma.app/onboard".into());
    // FRAGMENT (#claim=...) — the secret is NEVER logged.
    let onboard_link = format!("{base}#claim={claim_id}.{secret}");

    send_message(
        env,
        chat_id,
        "Оплата прошла успешно! Нажмите кнопку, чтобы активировать подписку в приложении.",
        Some(inline_keyboard_url("Активировать подписку", &onboard_link)),
    )
    .await?;

    // Record delivery (idempotent; re-delivery just re-stamps and re-sends).
    do_post(&stub, "/claims/mark-notified", &serde_json::json!({ "claimId": claim_id })).await?;

    Response::from_json(&serde_json::json!({ "ok": true }))
}

/// Mini App branch of /internal/paid. The Mini App claim is keyed by the Telegram
/// WebApp user.id; for a private chat with the bot `chat_id == user.id`, so we message
/// that id directly. The button is a web_app button that REOPENS the Mini App (NOT a raw
/// onboard URL): a URL button opens Telegram's in-app browser, where passkey/WebAuthn
/// fails — the Mini App's openLink → Safari is the working path. Idempotent via
/// miniapp_claims.notified_at. The claim secret is NEVER sent here or logged.
async fn internal_paid_miniapp(
    env: &Env,
    stub: &worker::durable::Stub,
    claim_id: &str,
) -> Result<Response> {
    let mut got = do_post(
        stub,
        "/miniapp/claims/get",
        &serde_json::json!({ "claimId": claim_id }),
    )
    .await?;
    let cv: serde_json::Value = got.json().await?;
    if cv.get("found").and_then(|v| v.as_bool()) == Some(false) {
        // Neither a chat claim nor a Mini App claim → genuinely unknown. 200 no-op.
        return Response::from_json(&serde_json::json!({ "ok": true, "skipped": "unknown_claim" }));
    }
    if cv.get("notifiedAt").and_then(|v| v.as_i64()).is_some() {
        return Response::from_json(&serde_json::json!({ "ok": true, "skipped": "already_notified" }));
    }
    let tg_user_id = match cv.get("tgUserId").and_then(|v| v.as_i64()) {
        Some(id) => id,
        None => return Ok(error_response("claim missing tgUserId", 500)),
    };

    send_message(
        env,
        tg_user_id,
        "Оплата прошла успешно! Откройте приложение, чтобы получить доступ к re:Norma.",
        Some(miniapp::inline_keyboard_web_app(
            "Открыть re:Norma",
            "https://tg.renorma.app/",
        )),
    )
    .await?;

    do_post(
        stub,
        "/miniapp/claims/mark-notified",
        &serde_json::json!({ "claimId": claim_id }),
    )
    .await?;

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

// Telegram Mini App (Web App) pay flow — served and driven entirely by telegram-worker.
//
// One screen: a promo-code field + «Оплатить». Pressing it creates a guest checkout via
// payment-worker /internal/checkout (carrying the promo), opens the lava payUrl in the
// SYSTEM browser via Telegram.WebApp.openLink, then POLLS /miniapp/status every ~3s. When
// the SIGNED lava webhook has marked the claim paid, the poll returns the onboard URL and
// the Mini App shows «Создать аккаунт» → openLink → Safari → /onboard#claim=<id>.<secret>.
//
// MONEY-SAFETY:
//  - `paid` is set ONLY by the signed lava webhook (payment-worker). This flow only READS
//    GET /claim/status; it never marks paid.
//  - The claim secret leaves payment-worker only via INTERNAL_PUSH_KEY /internal/checkout
//    and leaves telegram-worker only in the /miniapp/status response, ONLY after
//    status∈{paid,claimed}, ONLY to the initData-validated OWNER of the claim. Never logged.
//  - initData is validated (HMAC-SHA256 + 24h freshness) on EVERY /miniapp/* call.
//  - One user cannot read another user's claim (owner check by tg_user_id).

use worker::*;

use crate::init_data::{validate_init_data, InitDataOk};
use crate::{call_internal_checkout, error_response, token};

// ── GET / : the Mini App page ───────────────────────────────────────────────────
pub fn serve_miniapp_page(env: &Env) -> Result<Response> {
    // The out-of-Telegram gate's «Открыть в Telegram» link is env-driven (bot deep-link),
    // injected here so nothing is hardcoded.
    let pay_url = env.var("MINIAPP_PAY_URL").map(|v| v.to_string()).unwrap_or_default();
    let html = MINIAPP_HTML.replace("__MINIAPP_PAY_URL__", &pay_url);
    let headers = Headers::new();
    headers.set("Content-Type", "text/html; charset=utf-8")?;
    // CSP: page loads telegram-web-app.js from telegram.org + one inline <script>;
    // all API calls are same-origin /miniapp/*.
    headers.set(
        "Content-Security-Policy",
        "default-src 'self'; \
         script-src 'self' https://telegram.org 'unsafe-inline'; \
         connect-src 'self'; \
         img-src 'self' data:; \
         style-src 'self' 'unsafe-inline'",
    )?;
    // The Telegram in-app WebView caches aggressively; no-store makes sure a redeploy of
    // the UI is picked up on the next open instead of serving a stale version.
    headers.set("Cache-Control", "no-store")?;
    Ok(Response::ok(html)?.with_headers(headers))
}

// ── helper: read initData from JSON field (preferred) or header fallback ─────────
fn extract_init_data(body: &serde_json::Value, req: &Request) -> String {
    if let Some(s) = body.get("initData").and_then(|v| v.as_str()) {
        if !s.is_empty() {
            return s.to_string();
        }
    }
    req.headers()
        .get("X-Telegram-Init-Data")
        .ok()
        .flatten()
        .unwrap_or_default()
}

/// Resolve the bot token (fail-loud → 503) and validate initData (→ 401). Returns the
/// validated identity. The raw initData and secret_key are NEVER logged.
async fn require_init_data(
    env: &Env,
    init_data: &str,
) -> std::result::Result<InitDataOk, Response> {
    let token = match token::secret_or_var(env, "TELEGRAM_BOT_TOKEN").await {
        Ok(t) => t,
        Err(reason) => {
            console_error!("require_init_data: {reason}");
            return Err(error_response("misconfigured", 503));
        }
    };
    let now_ms = Date::now().as_millis() as i64;
    match validate_init_data(init_data, &token, now_ms) {
        Ok(ok) => Ok(ok),
        Err(reason) => {
            // Log only the category (first token) — never the raw initData/values.
            let cat = reason.split(':').next().unwrap_or("err");
            console_error!("require_init_data: rejected ({cat}); initData_len={}", init_data.len());
            Err(error_response("unauthorized", 401))
        }
    }
}

// ── POST /miniapp/checkout ───────────────────────────────────────────────────────
pub async fn miniapp_checkout(mut req: Request, env: &Env) -> Result<Response> {
    let body: serde_json::Value = req.json().await.unwrap_or(serde_json::json!({}));
    let init_data = extract_init_data(&body, &req);

    // [SEC #1] initData validation on every /miniapp/* call.
    let identity = match require_init_data(env, &init_data).await {
        Ok(id) => id,
        Err(resp) => return Ok(resp),
    };
    let tg_user_id = identity.tg_user_id;

    // Promo is optional (absent/empty → no promo, full price).
    let promo_code = body
        .get("promoCode")
        .and_then(|v| v.as_str())
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .map(String::from);

    // Buyer currency (RUB/USD/EUR) — REQUIRED, the wizard always sends it. Empty/missing → 400.
    let currency = match body
        .get("currency")
        .and_then(|v| v.as_str())
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
    {
        Some(c) => c.to_string(),
        None => {
            return Response::from_json(&serde_json::json!({ "error": "currency_required" }))
                .map(|r| r.with_status(400))
        }
    };
    // Buyer payment method (the wizard always sends CARD) — REQUIRED. Empty/missing → 400.
    let payment_method = match body
        .get("paymentMethod")
        .and_then(|v| v.as_str())
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
    {
        Some(m) => m.to_string(),
        None => {
            return Response::from_json(&serde_json::json!({ "error": "payment_method_required" }))
                .map(|r| r.with_status(400))
        }
    };

    // INTERNAL_PUSH_KEY-gated on the payment side. On failure: log loudly (no secret in
    // message), 502, store NO claim. The Telegram identity rides along so the claim
    // records WHO paid (operator reconciliation). Which lava offer to sell is decided by
    // payment-worker (LAVA_OFFER_ID) — no plan id passed from here.
    let checkout = match call_internal_checkout(
        env,
        promo_code.as_deref(),
        tg_user_id,
        identity.username.as_deref(),
        &currency,
        &payment_method,
    )
    .await
    {
        Ok(c) => c,
        Err(e) => {
            let es = e.to_string();
            // Already paid/claimed for this Telegram user → tell the client to re-render
            // into its access state instead of opening a duplicate invoice.
            if es.contains("ALREADY_ACTIVE") {
                return Response::from_json(&serde_json::json!({ "alreadyActive": true }));
            }
            console_error!("miniapp_checkout: internal/checkout failed: {e}");
            // Map lava's actual reason to a human message so the client shows WHY, not a
            // generic error. The English phrases below appear verbatim in lava's 400 body.
            let msg = if es.contains("Restricted payment method") {
                "Этот способ оплаты недоступен для выбранной валюты."
            } else if es.contains("can't be bought using") {
                "Этот способ оплаты недоступен для подписки."
            } else if es.contains("PaymentMethodType") {
                "Недопустимый способ оплаты."
            } else if es.to_lowercase().contains("promo") || es.to_lowercase().contains("coupon") {
                "Промокод не найден или недоступен для этой оплаты."
            } else if promo_code.is_some() {
                "Не удалось применить промокод. Проверьте код или другой способ оплаты."
            } else {
                "Не удалось создать счёт. Попробуйте другой способ оплаты."
            };
            return Response::from_json(&serde_json::json!({ "error": "checkout_failed", "message": msg }))
                .map(|r| r.with_status(400));
        }
    };

    // Ownership (claim → tg user + secret) is now written by payment-worker directly into
    // ClaimDO (tg_claims) at checkout — single source of truth, no local store to drift.

    // [SEC #3] Respond with claimId + payUrl ONLY — never the secret. amount/currency
    // are the lava-decoded price of the CREATED invoice (promo-applied) so the client
    // shows the discounted/actual price from server values; null when the decode missed.
    Response::from_json(&serde_json::json!({
        "claimId": checkout.claim_id,
        "payUrl": checkout.pay_url,
        "amount": checkout.amount,
        "currency": checkout.currency,
        // Invoice lifetime (ms) for the client's expiry watch on the final checkout.
        "ttlMs": checkout.ttl_ms,
    }))
}

// ── POST /miniapp/price ────────────────────────────────────────────────────────
/// The offer's list price for the requested currency, WITHOUT minting an invoice — the
/// Mini App "ценник" before any promo. initData-gated; proxies payment-worker
/// /internal/price. Returns {amount, currency}; amount is null → client shows "…".
pub async fn miniapp_price(mut req: Request, env: &Env) -> Result<Response> {
    let body: serde_json::Value = req.json().await.unwrap_or(serde_json::json!({}));
    let init_data = extract_init_data(&body, &req);
    if let Err(resp) = require_init_data(env, &init_data).await {
        return Ok(resp);
    }
    let currency = match body
        .get("currency")
        .and_then(|v| v.as_str())
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
    {
        Some(c) => c,
        None => {
            return Response::from_json(&serde_json::json!({ "error": "currency_required" }))
                .map(|r| r.with_status(400))
        }
    };

    let key = token::secret_or_var(env, "INTERNAL_PUSH_KEY")
        .await
        .map_err(Error::RustError)?;
    let payload = serde_json::json!({ "currency": currency }).to_string();
    let headers = Headers::new();
    headers.set("Content-Type", "application/json")?;
    headers.set("X-Internal-Key", &key)?;
    let mut init = RequestInit::new();
    init.with_method(Method::Post)
        .with_headers(headers)
        .with_body(Some(wasm_bindgen::JsValue::from_str(&payload)));
    let request = Request::new_with_init("https://payment-worker/internal/price", &init)?;
    let payment = env
        .service("PAYMENT_WORKER")
        .map_err(|e| Error::RustError(format!("PAYMENT_WORKER binding: {e}")))?;
    let mut resp = payment.fetch_request(request).await?;
    let sc = resp.status_code();
    if !(200..300).contains(&sc) {
        let txt = resp.text().await.unwrap_or_default();
        console_error!("miniapp_price: internal/price {sc}: {txt}");
        return Ok(error_response("price_unavailable", 502));
    }
    let v: serde_json::Value = resp.json().await?;
    Response::from_json(&v)
}

// ── POST /miniapp/status ─────────────────────────────────────────────────────────
pub async fn miniapp_status(mut req: Request, env: &Env) -> Result<Response> {
    let body: serde_json::Value = req.json().await.unwrap_or(serde_json::json!({}));
    let init_data = extract_init_data(&body, &req);

    // [SEC #1] initData validation.
    let tg_user_id = match require_init_data(env, &init_data).await {
        Ok(id) => id.tg_user_id,
        Err(resp) => return Ok(resp),
    };

    let claim_id = body
        .get("claimId")
        .and_then(|v| v.as_str())
        .map(|s| s.trim())
        .filter(|s| !s.is_empty());
    let claim_id = match claim_id {
        Some(c) => c.to_string(),
        None => return Ok(error_response("missing claimId", 400)),
    };

    // Owner lookup in payment-worker (tg_claims). Unknown claim → {status:"none"}.
    let cv = payment_tg(env, "get", serde_json::json!({ "claimId": claim_id })).await?;
    if cv.get("found").and_then(|v| v.as_bool()) != Some(true) {
        return Response::from_json(&serde_json::json!({ "status": "none" }));
    }
    // [SEC #4] Owner check: one user can't read another user's claim/secret.
    let owner = cv.get("tgId").and_then(|v| v.as_i64());
    if owner != Some(tg_user_id) {
        return Ok(error_response("forbidden", 403));
    }
    let secret = match cv.get("secret").and_then(|v| v.as_str()) {
        Some(s) => s.to_string(),
        None => return Ok(error_response("claim missing secret", 500)),
    };

    // READ-only status from payment-worker (never marks paid).
    let status = match fetch_claim_status(env, &claim_id).await {
        Ok(s) => s,
        Err(e) => {
            console_error!("miniapp_status: fetch_claim_status failed: {e}");
            return Ok(error_response("status_unavailable", 502));
        }
    };

    // Release the onboard link ONLY on paid/claimed, ONLY to the validated owner.
    if status == "paid" || status == "claimed" {
        let base = env
            .var("APP_ONBOARD_URL")
            .map(|v| v.to_string())
            .unwrap_or_default(); // APP_ONBOARD_URL is set in both envs; no hardcoded fallback.
        // NEW MODEL: `?u=<user_id>` (non-secret) → the code login on /onboard uses it. Legacy
        // fragment only if the account resolve hiccuped.
        let onboard_url = match resolve_user_id(env, tg_user_id, None).await {
            Some(uid) => match mint_code(env, &uid).await {
                Some(code) => format!("{base}?u={uid}#code={code}"),
                None => format!("{base}?u={uid}"),
            },
            None => format!("{base}#claim={claim_id}.{secret}"),
        };
        Response::from_json(&serde_json::json!({
            "status": status,
            "onboardUrl": onboard_url,
        }))
    } else {
        // pending / void / none → no secret, no onboardUrl.
        Response::from_json(&serde_json::json!({ "status": status }))
    }
}

// ── POST /miniapp/me ───────────────────────────────────────────────────────────
/// Per-user persistence: does THIS Telegram user already have a paid subscription?
/// Looks up the user's claims (newest first) and returns the onboard URL of the first
/// paid/claimed one — so a returning paid user sees «Получить доступ к re:Norma», not
/// the pay form. No paid claim → {status:"none"}. Same money/secret-safety model as
/// /miniapp/status: secret released ONLY after payment-worker confirms paid/claimed.
pub async fn miniapp_me(mut req: Request, env: &Env) -> Result<Response> {
    let body: serde_json::Value = req.json().await.unwrap_or(serde_json::json!({}));
    let init_data = extract_init_data(&body, &req);

    // [SEC #1] initData validation.
    let identity = match require_init_data(env, &init_data).await {
        Ok(id) => id,
        Err(resp) => return Ok(resp),
    };
    let tg_user_id = identity.tg_user_id;

    // FIRST TOUCH: resolve-or-create the universal user_id for this Telegram identity
    // (idempotent). Returned to the Mini App so it can carry `?u=<user_id>` into the onboarding
    // link + the dynamic manifest. Best-effort — a hiccup just omits it.
    let account_user_id =
        resolve_user_id(env, tg_user_id, identity.username.as_deref()).await;

    // The user's claims, newest first (from payment-worker's tg_claims; secrets in-process).
    let cv = payment_tg(env, "by-user", serde_json::json!({ "tgId": tg_user_id })).await?;
    let empty = vec![];
    let claims = cv.get("claims").and_then(|v| v.as_array()).unwrap_or(&empty);

    // First paid/claimed claim wins → release its onboard URL.
    for claim in claims {
        let claim_id = match claim.get("claimId").and_then(|v| v.as_str()) {
            Some(c) => c,
            None => continue,
        };
        let secret = match claim.get("secret").and_then(|v| v.as_str()) {
            Some(s) => s,
            None => continue,
        };
        let status = match fetch_claim_status(env, claim_id).await {
            Ok(s) => s,
            Err(e) => {
                console_error!("miniapp_me: fetch_claim_status failed: {e}");
                continue; // a transient lookup error on one claim must not hide the rest
            }
        };
        if status == "paid" || status == "claimed" {
            let base = env
                .var("APP_ONBOARD_URL")
                .map(|v| v.to_string())
                .unwrap_or_default(); // APP_ONBOARD_URL is set in both envs; no hardcoded fallback.
            // NEW MODEL: carry the universal user_id as `?u=` — the code login on /onboard
            // uses it (it's non-secret). Legacy fragment (#claim=…) only if the account
            // resolve hiccuped, so onboarding still works during the transition.
            let onboard_url = match &account_user_id {
                Some(uid) => match mint_code(env, uid).await {
                    Some(code) => format!("{base}?u={uid}#code={code}"),
                    None => format!("{base}?u={uid}"),
                },
                None => format!("{base}#claim={claim_id}.{secret}"),
            };
            // App root (for an already-registered user → «Открыть приложение»). Derived from
            // APP_ONBOARD_URL by dropping "/onboard".
            let app_url = base.strip_suffix("/onboard").unwrap_or(&base).to_string();
            // «Access obtained» = the user reached the app (first story chapter available), NOT
            // the payment status. That decides «Открыть приложение» vs «Получить доступ» — the
            // button must reflect whether the user actually got into the app.
            let has_access = match &account_user_id {
                Some(uid) => has_entered(env, uid).await,
                None => false,
            };
            let mut out = serde_json::json!({
                "status": status,
                "hasAccess": has_access,
                "onboardUrl": onboard_url,
                "appUrl": app_url,
                "userId": account_user_id.clone(),
            });
            // Live subscription of the bound account (if onboarded) → active/cancelled
            // + days. Best-effort: a lookup error just omits it (the access still works).
            if let Ok(Some(s)) = fetch_claim_subscription(env, claim_id).await {
                out["subStatus"] = s.get("subStatus").cloned().unwrap_or(serde_json::Value::Null);
                out["daysLeft"] = s.get("daysLeft").cloned().unwrap_or(serde_json::Value::Null);
                out["noRenew"] = s.get("noRenew").cloned().unwrap_or(serde_json::Value::Null);
            }
            return Response::from_json(&out);
        }
    }

    // No paid/claimed claim. Surface a pending invoice (if any) with its deadline so the
    // Mini App shows «pay invoice until <deadline>» while valid, or «create new invoice»
    // once expired. Best-effort: a lookup error just falls back to the plain pay form.
    match fetch_pending_by_tg(env, tg_user_id).await {
        Ok(Some(p)) => Response::from_json(&serde_json::json!({
            "status": "none",
            "userId": account_user_id.clone(),
            "pendingInvoice": {
                "claimId": p.get("claimId"),
                "payUrl": p.get("payUrl"),
                "deadline": p.get("deadline"),
                "expired": p.get("expired"),
                "lavaPaid": p.get("lavaPaid"),
            },
        })),
        _ => Response::from_json(&serde_json::json!({ "status": "none", "userId": account_user_id })),
    }
}

// ── POST /miniapp/access-link ────────────────────────────────────────────────────
/// Mint a FRESH one-time code on every call and return the onboarding URL carrying it. The
/// «Получить доступ к re:Norma» button calls this on each press so the code in the opened link
/// is always brand-new (codes are single-use + expire in 10 min) — no stale/consumed code.
/// The Mini App proved the Telegram identity via initData, so it's the trusted owner.
pub async fn miniapp_access_link(mut req: Request, env: &Env) -> Result<Response> {
    let body: serde_json::Value = req.json().await.unwrap_or(serde_json::json!({}));
    let init_data = extract_init_data(&body, &req);

    // [SEC #1] initData validation on every /miniapp/* call.
    let identity = match require_init_data(env, &init_data).await {
        Ok(id) => id,
        Err(resp) => return Ok(resp),
    };

    let user_id = match resolve_user_id(env, identity.tg_user_id, identity.username.as_deref()).await {
        Some(uid) => uid,
        None => return Ok(error_response("account_unavailable", 502)),
    };
    let code = match mint_code(env, &user_id).await {
        Some(c) => c,
        None => return Ok(error_response("code_unavailable", 502)),
    };
    let base = env
        .var("APP_ONBOARD_URL")
        .map(|v| v.to_string())
        .unwrap_or_default(); // APP_ONBOARD_URL is set in both envs; no hardcoded fallback.
    Response::from_json(&serde_json::json!({
        "onboardUrl": format!("{base}?u={user_id}#code={code}"),
    }))
}

/// Resolve-or-create the universal user_id for a Telegram identity via the AUTH_WORKER
/// binding (first touch; idempotent). Best-effort → None on any failure (never blocks the
/// Mini App). Provider-agnostic on the auth side; here the provider is always "telegram".
async fn resolve_user_id(env: &Env, tg_user_id: i64, username: Option<&str>) -> Option<String> {
    let key = token::secret_or_var(env, "INTERNAL_PUSH_KEY").await.ok()?;
    let mut body =
        serde_json::json!({ "provider": "telegram", "providerUid": tg_user_id.to_string() });
    if let Some(u) = username {
        body["username"] = serde_json::Value::String(u.to_string());
    }
    let headers = Headers::new();
    headers.set("Content-Type", "application/json").ok()?;
    headers.set("X-Internal-Key", &key).ok()?;
    let mut init = RequestInit::new();
    init.with_method(Method::Post)
        .with_headers(headers)
        .with_body(Some(wasm_bindgen::JsValue::from_str(&body.to_string())));
    let request =
        Request::new_with_init("https://auth-worker/internal/account-resolve", &init).ok()?;
    let auth = env.service("AUTH_WORKER").ok()?;
    let mut res = auth.fetch_request(request).await.ok()?;
    if !(200..300).contains(&res.status_code()) {
        console_error!("resolve_user_id: auth {}", res.status_code());
        return None;
    }
    let v: serde_json::Value = res.json().await.ok()?;
    v.get("userId").and_then(|x| x.as_str()).map(String::from)
}

/// Mint a one-time code for `user_id` via the AUTH_WORKER binding and return it (no delivery).
/// The Mini App already proved the Telegram identity (initData), so it's the trusted owner —
/// the code is embedded in the onboard link so onboarding can auto-authorize. None on failure.
async fn mint_code(env: &Env, user_id: &str) -> Option<String> {
    let key = token::secret_or_var(env, "INTERNAL_PUSH_KEY").await.ok()?;
    let payload = serde_json::json!({ "userId": user_id }).to_string();
    let headers = Headers::new();
    headers.set("Content-Type", "application/json").ok()?;
    headers.set("X-Internal-Key", &key).ok()?;
    let mut init = RequestInit::new();
    init.with_method(Method::Post)
        .with_headers(headers)
        .with_body(Some(wasm_bindgen::JsValue::from_str(&payload)));
    let request = Request::new_with_init("https://auth-worker/internal/code/mint", &init).ok()?;
    let auth = env.service("AUTH_WORKER").ok()?;
    let mut res = auth.fetch_request(request).await.ok()?;
    if !(200..300).contains(&res.status_code()) {
        return None;
    }
    let v: serde_json::Value = res.json().await.ok()?;
    v.get("code").and_then(|x| x.as_str()).map(String::from)
}

/// Has this account «entered the system» — i.e. reached the running app so the FIRST story
/// chapter became available? That's the definition of «access obtained» (works for both passkey
/// and code-only users, unlike a passkey count). Via AUTH_WORKER /internal/has-entered.
/// Best-effort → false on any failure (worst case the Mini App shows «Получить доступ»).
async fn has_entered(env: &Env, user_id: &str) -> bool {
    async fn inner(env: &Env, user_id: &str) -> Option<bool> {
        let key = token::secret_or_var(env, "INTERNAL_PUSH_KEY").await.ok()?;
        let payload = serde_json::json!({ "userId": user_id }).to_string();
        let headers = Headers::new();
        headers.set("Content-Type", "application/json").ok()?;
        headers.set("X-Internal-Key", &key).ok()?;
        let mut init = RequestInit::new();
        init.with_method(Method::Post)
            .with_headers(headers)
            .with_body(Some(wasm_bindgen::JsValue::from_str(&payload)));
        let request =
            Request::new_with_init("https://auth-worker/internal/has-entered", &init).ok()?;
        let auth = env.service("AUTH_WORKER").ok()?;
        let mut res = auth.fetch_request(request).await.ok()?;
        if !(200..300).contains(&res.status_code()) {
            return None;
        }
        let v: serde_json::Value = res.json().await.ok()?;
        v.get("entered").and_then(|x| x.as_bool())
    }
    inner(env, user_id).await.unwrap_or(false)
}

/// GET payment-worker /claim/status?claimId=… over the PAYMENT_WORKER service binding.
/// Public route (no internal key); our gate is the initData validation + owner check.
/// Non-2xx → Err. Parses {status}; default "none".
async fn fetch_claim_status(env: &Env, claim_id: &str) -> Result<String> {
    let mut init = RequestInit::new();
    init.with_method(Method::Get);
    let enc = js_sys::encode_uri_component(claim_id)
        .as_string()
        .unwrap_or_default();
    let url = format!("https://payment-worker/claim/status?claimId={enc}");
    let request = Request::new_with_init(&url, &init)?;
    let payment = env
        .service("PAYMENT_WORKER")
        .map_err(|e| Error::RustError(format!("PAYMENT_WORKER binding: {e}")))?;
    let mut resp = payment.fetch_request(request).await?;
    let status_code = resp.status_code();
    if !(200..300).contains(&status_code) {
        let txt = resp.text().await.unwrap_or_default();
        return Err(Error::RustError(format!(
            "claim/status {status_code}: {txt}"
        )));
    }
    let v: serde_json::Value = resp.json().await?;
    let status = v
        .get("status")
        .and_then(|x| x.as_str())
        .unwrap_or("none")
        .to_string();
    Ok(status)
}

/// POST payment-worker /internal/claim-subscription (INTERNAL_PUSH_KEY) → the LIVE
/// subscription of the account this claim is bound to (active/cancelled + days), or
/// None if the claim was paid but not yet onboarded to an account.
async fn fetch_claim_subscription(env: &Env, claim_id: &str) -> Result<Option<serde_json::Value>> {
    let key = token::secret_or_var(env, "INTERNAL_PUSH_KEY")
        .await
        .map_err(Error::RustError)?;
    let payload = serde_json::json!({ "claimId": claim_id }).to_string();
    let headers = Headers::new();
    headers.set("Content-Type", "application/json")?;
    headers.set("X-Internal-Key", &key)?;
    let mut init = RequestInit::new();
    init.with_method(Method::Post)
        .with_headers(headers)
        .with_body(Some(wasm_bindgen::JsValue::from_str(&payload)));
    let request =
        Request::new_with_init("https://payment-worker/internal/claim-subscription", &init)?;
    let payment = env
        .service("PAYMENT_WORKER")
        .map_err(|e| Error::RustError(format!("PAYMENT_WORKER binding: {e}")))?;
    let mut resp = payment.fetch_request(request).await?;
    let sc = resp.status_code();
    if !(200..300).contains(&sc) {
        let txt = resp.text().await.unwrap_or_default();
        return Err(Error::RustError(format!("claim-subscription {sc}: {txt}")));
    }
    let v: serde_json::Value = resp.json().await?;
    if v.get("bound").and_then(|b| b.as_bool()) == Some(true) {
        Ok(Some(v))
    } else {
        Ok(None)
    }
}

/// POST payment-worker /internal/active-by-tg (INTERNAL_PUSH_KEY) → the user's newest
/// pending invoice with its deadline + expired flag, so the Mini App can offer «pay the
/// existing invoice until <deadline>» or «create a new invoice» once it's expired.
/// `{pending:false}` (or an error) → no payable invoice to surface.
async fn fetch_pending_by_tg(env: &Env, tg_user_id: i64) -> Result<Option<serde_json::Value>> {
    let key = token::secret_or_var(env, "INTERNAL_PUSH_KEY")
        .await
        .map_err(Error::RustError)?;
    let payload = serde_json::json!({ "tgUserId": tg_user_id }).to_string();
    let headers = Headers::new();
    headers.set("Content-Type", "application/json")?;
    headers.set("X-Internal-Key", &key)?;
    let mut init = RequestInit::new();
    init.with_method(Method::Post)
        .with_headers(headers)
        .with_body(Some(wasm_bindgen::JsValue::from_str(&payload)));
    let request = Request::new_with_init("https://payment-worker/internal/active-by-tg", &init)?;
    let payment = env
        .service("PAYMENT_WORKER")
        .map_err(|e| Error::RustError(format!("PAYMENT_WORKER binding: {e}")))?;
    let mut resp = payment.fetch_request(request).await?;
    let sc = resp.status_code();
    if !(200..300).contains(&sc) {
        let txt = resp.text().await.unwrap_or_default();
        return Err(Error::RustError(format!("active-by-tg {sc}: {txt}")));
    }
    let v: serde_json::Value = resp.json().await?;
    if v.get("pending").and_then(|b| b.as_bool()) == Some(true) {
        Ok(Some(v))
    } else {
        Ok(None)
    }
}

/// Call payment-worker /internal/tg/{op} (INTERNAL_PUSH_KEY) — the Telegram binding +
/// claim secret now live in payment-worker's ClaimDO, so telegram-worker reads/marks them
/// there instead of a local store. Returns the raw JSON.
pub(crate) async fn payment_tg(env: &Env, op: &str, body: serde_json::Value) -> Result<serde_json::Value> {
    let key = token::secret_or_var(env, "INTERNAL_PUSH_KEY")
        .await
        .map_err(Error::RustError)?;
    let payload = body.to_string();
    let headers = Headers::new();
    headers.set("Content-Type", "application/json")?;
    headers.set("X-Internal-Key", &key)?;
    let mut init = RequestInit::new();
    init.with_method(Method::Post)
        .with_headers(headers)
        .with_body(Some(wasm_bindgen::JsValue::from_str(&payload)));
    let request = Request::new_with_init(&format!("https://payment-worker/internal/tg/{op}"), &init)?;
    let payment = env
        .service("PAYMENT_WORKER")
        .map_err(|e| Error::RustError(format!("PAYMENT_WORKER binding: {e}")))?;
    let mut resp = payment.fetch_request(request).await?;
    let sc = resp.status_code();
    if !(200..300).contains(&sc) {
        let txt = resp.text().await.unwrap_or_default();
        return Err(Error::RustError(format!("tg/{op} {sc}: {txt}")));
    }
    resp.json().await
}

// ── helpers shared with lib.rs ──────────────────────────────────────────────────
/// inline keyboard with a web_app button (Telegram opens the Mini App). Parallels
/// inline_keyboard_url, but Telegram needs `web_app`, not `url`.
#[allow(dead_code)]
pub fn inline_keyboard_web_app(text: &str, url: &str) -> serde_json::Value {
    serde_json::json!({
        "inline_keyboard": [[ { "text": text, "web_app": { "url": url } } ]]
    })
}

// ── static Mini App HTML (one screen, RU copy) ──────────────────────────────────
const MINIAPP_HTML: &str = r##"<!DOCTYPE html>
<html lang="ru">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1, viewport-fit=cover">
<title>Renorma — оплата</title>
<script src="https://telegram.org/js/telegram-web-app.js"></script>
<style>
  :root {
    color-scheme: light dark;
    --accent: #10B981;
    --accent-2: #059669;
    --accent-soft: rgba(16,185,129,0.12);
    --accent-glow: rgba(16,185,129,0.32);
    --bg: var(--tg-theme-bg-color, #ffffff);
    --ink: var(--tg-theme-text-color, #0E1630);
    --muted: var(--tg-theme-hint-color, #8b909a);
    --surface: var(--tg-theme-secondary-bg-color, #f4f5f7);
    --line: rgba(120,130,150,0.22);
  }
  * { box-sizing: border-box; }
  body {
    margin: 0;
    font-family: -apple-system, BlinkMacSystemFont, "SF Pro Display", "Segoe UI", Roboto, sans-serif;
    -webkit-font-smoothing: antialiased; text-rendering: optimizeLegibility;
    background: var(--bg); color: var(--ink);
    min-height: 100vh; display: flex; align-items: center; justify-content: center;
    padding: 26px 20px;
  }
  .card { width: 100%; max-width: 400px; display: flex; flex-direction: column; gap: 24px; }

  /* brand */
  .brand { display: flex; align-items: center; gap: 14px; }
  .logo { width: 54px; height: 54px; border-radius: 16px; background: #0E1630;
    display: flex; align-items: center; justify-content: center; flex: 0 0 auto;
    box-shadow: 0 10px 26px rgba(14,22,48,0.30); }
  .ring { width: 34px; height: 34px; border-radius: 50%;
    background: conic-gradient(#10B981 0 60%, #F43F5E 60% 90%, #FFF200 90% 100%);
    -webkit-mask: radial-gradient(circle, transparent 8px, #000 9px);
    mask: radial-gradient(circle, transparent 8px, #000 9px); }
  h1 { font-size: 21px; margin: 0; font-weight: 800; letter-spacing: -0.02em; }
  p.sub { margin: 0; font-size: 14px; line-height: 1.5; color: var(--muted); }

  /* wizard shell */
  #wizard { display: flex; flex-direction: column; gap: 22px; }
  #progress { display: flex; gap: 6px; }
  #progress .dot { height: 4px; flex: 1; border-radius: 4px; background: var(--line); transition: background .25s; }
  #progress .dot.on { background: var(--accent); }
  .step { display: flex; flex-direction: column; gap: 14px; animation: rise .30s cubic-bezier(.2,.7,.2,1) both; }
  @keyframes rise { from { opacity: 0; transform: translateY(10px); } to { opacity: 1; transform: none; } }
  .steptitle { font-size: 24px; font-weight: 800; margin: 2px 0 0; letter-spacing: -0.03em; }

  /* option buttons (world / currency) */
  .optbtns { display: flex; gap: 10px; }
  .optbtns.col { flex-direction: column; }
  .opt { width: 100%; margin: 0; text-align: left; padding: 17px 18px;
    font-size: 16px; font-weight: 600; color: var(--ink);
    background: var(--surface); border: 1.5px solid transparent; border-radius: 15px;
    box-shadow: none; cursor: pointer; position: relative;
    transition: border-color .15s, background .15s, transform .06s; }
  .opt:active { transform: scale(0.985); }
  .opt.active { border-color: var(--accent); background: var(--accent-soft); }
  .opt.active::after { content: "✓"; position: absolute; right: 16px; top: 50%;
    transform: translateY(-50%); color: var(--accent); font-weight: 800; font-size: 16px; }

  /* price card */
  .pricecard { display: flex; flex-direction: column; align-items: center; gap: 3px;
    padding: 22px 18px; background: var(--surface); border-radius: 18px; }
  .pricelabel { font-size: 12px; color: var(--muted); text-transform: uppercase;
    letter-spacing: 0.08em; font-weight: 700; }
  #priceLine { font-size: 36px; font-weight: 800; letter-spacing: -0.02em; color: var(--accent); line-height: 1.1; }
  .priceper { font-size: 13px; color: var(--muted); }

  /* input */
  input { width: 100%; padding: 15px 16px; font-size: 16px; border-radius: 14px;
    border: 1.5px solid var(--line); background: var(--surface); color: var(--ink); }
  input::placeholder { color: var(--muted); }
  input:focus { outline: none; border-color: var(--accent); }
  #appliedPromo { align-self: flex-start; font-size: 14px; font-weight: 700; color: var(--accent);
    background: var(--accent-soft); padding: 10px 15px; border-radius: 999px; }

  /* summary */
  .summary { display: flex; flex-direction: column; gap: 13px;
    background: var(--surface); padding: 18px; border-radius: 18px; }
  .summary .row { display: flex; justify-content: space-between; align-items: baseline; font-size: 15px; }
  .summary .row .k { color: var(--muted); }
  .summary .row .v { font-weight: 700; text-align: right; }
  .summary .row.total { border-top: 1px solid var(--line); padding-top: 14px; margin-top: 3px; }
  .summary .row.total .k { color: var(--ink); font-weight: 700; }
  .summary .row.total .v { font-size: 24px; font-weight: 800; color: var(--accent); }

  /* buttons */
  button { width: 100%; padding: 16px; font-size: 16px; font-weight: 700; border: none;
    border-radius: 14px; cursor: pointer; letter-spacing: -0.01em; color: #fff;
    background: linear-gradient(135deg, var(--accent), var(--accent-2));
    box-shadow: 0 10px 24px var(--accent-glow);
    transition: transform .06s, box-shadow .15s, opacity .15s; }
  button:active { transform: translateY(1px); box-shadow: 0 5px 14px var(--accent-glow); }
  button:disabled { opacity: 0.5; box-shadow: none; cursor: default; }
  button.secondary { background: transparent; color: var(--muted); border: 1.5px solid var(--line);
    box-shadow: none; font-weight: 600; }
  .nav { display: flex; gap: 10px; }
  .nav .secondary { flex: 0 0 34%; }

  /* status + spinner */
  .status { text-align: center; font-size: 14px; min-height: 20px; color: var(--muted); }
  .status.err { color: #ef4444; font-weight: 600; }
  .spinner { width: 30px; height: 30px; margin: 6px auto 0; border: 3px solid var(--line);
    border-top-color: var(--accent); border-radius: 50%; animation: spin 0.8s linear infinite; }
  .price-spin { display: inline-block; width: 28px; height: 28px; border: 3px solid var(--line);
    border-top-color: var(--accent); border-radius: 50%; animation: spin 0.8s linear infinite; }
  @keyframes spin { to { transform: rotate(360deg); } }

  .hidden { display: none !important; }

  /* gate */
  #gate { text-align: center; align-items: center; gap: 18px; }
  .tgbtn { display: block; text-decoration: none; text-align: center; padding: 16px;
    font-size: 16px; font-weight: 700; border-radius: 14px; color: #fff;
    background: linear-gradient(135deg, var(--accent), var(--accent-2));
    box-shadow: 0 10px 24px var(--accent-glow); }
</style>
</head>
<body>
<div class="card">
  <div class="brand">
    <span class="logo" aria-hidden="true"><span class="ring"></span></span>
    <div>
      <h1>Подписка re:Norma</h1>
    </div>
  </div>

  <p class="sub">re:Norma — приложение для нормализации веса, рациона питания и образа жизни. Подписка открывает полный доступ ко всем функциям.</p>

  <!-- WIZARD: one choice per step; Back/Next nav. The pay link is cached by the tuple
       world|currency|provider|promo — a repeat with the SAME tuple reuses the invoice,
       changing anything mints a fresh one. -->
  <div id="wizard">
    <div id="progress"></div>
    <div class="step" data-step="world">
      <p class="steptitle">Какая у вас карта?</p>
      <div id="worldSel" class="optbtns col">
        <button type="button" class="opt" data-world="RU">Российская карта</button>
        <button type="button" class="opt" data-world="INTL">Иностранная карта</button>
      </div>
    </div>

    <div class="step hidden" data-step="currency">
      <p class="steptitle">Валюта оплаты</p>
      <div id="curSel" class="optbtns col"></div>
    </div>

    <div class="step hidden" data-step="promo">
      <p class="steptitle">Промокод</p>
      <div class="pricecard">
        <span class="pricelabel">Стоимость подписки</span>
        <div id="priceLine">…</div>
        <span class="priceper">в месяц</span>
      </div>
      <p class="sub">Есть промокод — введите его. Нет — просто «Далее».</p>
      <input id="promo" type="text" autocomplete="off" autocapitalize="off" placeholder="Промокод">
    </div>

    <div class="step hidden" data-step="final">
      <p class="steptitle">Проверьте заказ</p>
      <div id="summary" class="summary"></div>
    </div>

    <div id="wizStatus" class="status"></div>
    <div id="wizSpinner" class="spinner hidden"></div>
    <div class="nav">
      <button id="backBtn" class="secondary hidden">Назад</button>
      <button id="nextBtn">Далее</button>
    </div>
  </div>

  <!-- Access state: this Telegram user already has a subscription. -->
  <div id="access" class="hidden">
    <div id="accessStatus" class="status"></div>
    <button id="createBtn">Открыть приложение</button>
  </div>

  <div id="claimInfo" class="sub" style="font-size:11px; word-break:break-all; text-align:left; white-space:pre-line;"></div>
  <div style="font-size:10px; opacity:0.5; text-align:center; margin-top:10px;">build: pay-r19</div>
</div>

<div id="gate" class="card hidden">
  <div class="brand" style="justify-content: center;">
    <span class="logo" aria-hidden="true"><span class="ring"></span></span>
    <div><h1>re:Norma</h1></div>
  </div>
  <p class="sub">Оформление подписки доступно только внутри Telegram.</p>
  <a class="tgbtn" href="__MINIAPP_PAY_URL__">Открыть в Telegram</a>
</div>

<script>
(function () {
  var tg = window.Telegram && window.Telegram.WebApp;
  if (tg) { tg.ready(); tg.expand(); }
  var initData = tg ? tg.initData : "";

  // Gate: this Mini App is meant to run ONLY inside Telegram, where a valid launch
  // carries signed initData. Opened in a plain browser (no initData) → show a notice
  // and NEVER the pay UI. (The privileged /miniapp/* endpoints already reject empty
  // initData server-side; this is the matching front-door.)
  if (!initData) {
    document.querySelector(".card").classList.add("hidden");
    document.getElementById("gate").classList.remove("hidden");
    return;
  }

  // ── DOM ─────────────────────────────────────────────────────────────────────
  var wizard = document.getElementById("wizard");
  var progress = document.getElementById("progress");
  var nav = document.querySelector("#wizard .nav");
  var accessEl = document.getElementById("access");
  var accessStatus = document.getElementById("accessStatus");
  var createBtn = document.getElementById("createBtn");
  var worldSel = document.getElementById("worldSel");
  var curSel = document.getElementById("curSel");
  var priceLine = document.getElementById("priceLine");
  var promoInput = document.getElementById("promo");
  var summaryEl = document.getElementById("summary");
  var wizStatus = document.getElementById("wizStatus");
  var wizSpinner = document.getElementById("wizSpinner");
  var backBtn = document.getElementById("backBtn");
  var nextBtn = document.getElementById("nextBtn");
  var stepEls = {};
  Array.prototype.forEach.call(document.querySelectorAll("#wizard .step"), function (s) {
    stepEls[s.getAttribute("data-step")] = s;
  });

  function show(el, on) { if (el) el.classList.toggle("hidden", !on); }

  function api(path, body) {
    body = body || {};
    body.initData = initData;
    return fetch(path, {
      method: "POST",
      headers: { "Content-Type": "application/json", "X-Telegram-Init-Data": initData },
      body: JSON.stringify(body)
    }).then(function (r) {
      // Always parse the body: a non-2xx carries a human `message` we want to surface.
      return r.json().catch(function () { return {}; }).then(function (j) {
        if (!r.ok) {
          var e = new Error(j.message || j.error || ("http " + r.status));
          e.userMessage = j.message || null;
          throw e;
        }
        return j;
      });
    });
  }

  function openLink(url) {
    if (tg && typeof tg.openLink === "function") { tg.openLink(url); }
    else { window.open(url, "_blank"); }
  }

  // Russian day-count declension: 1 день, 2 дня, 5 дней.
  function ruDays(n) {
    n = Math.abs(n); var d = n % 100, e = n % 10;
    if (d >= 11 && d <= 14) return "дней";
    if (e === 1) return "день";
    if (e >= 2 && e <= 4) return "дня";
    return "дней";
  }

  // Money formatter over SERVER values ONLY (never computes/discounts). `prefix`=true
  // renders «Цена: N ₽», false renders «N ₽» (for the summary total).
  function fmtMoney(amount, currency, prefix) {
    if (amount === null || amount === undefined) { return "…"; }
    var sym = currency === "USD" ? "$" : currency === "EUR" ? "€" : "₽";
    var n = amount;
    if (typeof n === "number" && Number.isFinite(n)) {
      n = (Math.round(n * 100) % 100 === 0) ? String(Math.round(n)) : n.toFixed(2);
    }
    return (prefix ? "Цена: " : "") + n + " " + sym;
  }

  // ── raw form answers (exactly what the user picked at each step) ─────────────────
  var selWorld = null;           // step 1: 'RU' | 'INTL'
  var selForeignCurrency = null; // step 2 (foreign only): 'USD' | 'EUR'
  var selProvider = "CARD";      // FIXED to CARD — the only reliable method across all
                                 // currencies (lava intermittently restricts SBP/STRIPE).
  var selPromo = "";             // promo step: trimmed promo (may be empty)
  // Cache the created pay link by the API tuple currency|method|promo: an unchanged tuple
  // reuses the cached link; changing anything mints a fresh invoice.
  var invoiceCache = {};
  var stepIdx = 0;
  var priceSeq = 0;
  var claimId = null;
  var accessUrl = null;
  var mintOnAccess = false;   // paid-but-not-onboarded → mint a FRESH code on each «Получить доступ»
  var pollTimer = null;
  var expiryTimer = null;

  // Steps in order — currency step only for foreign. No provider step (always CARD).
  function seq() {
    return selWorld === "INTL"
      ? ["world", "currency", "promo", "final"]
      : ["world", "promo", "final"];
  }

  function busy(on, msg) {
    show(wizSpinner, on);
    nextBtn.disabled = on; backBtn.disabled = on;
    wizStatus.classList.remove("err");
    wizStatus.textContent = msg || "";
  }
  function err(msg) { show(wizSpinner, false); nextBtn.disabled = false; backBtn.disabled = false; wizStatus.classList.add("err"); wizStatus.textContent = msg; }

  // Render a set of option buttons into a container. Picking one highlights it and calls
  // onPick(value) — no auto-advance (each step has its own «Далее»).
  function renderOptions(container, items, selected, onPick) {
    container.innerHTML = "";
    items.forEach(function (it) {
      var b = document.createElement("button");
      b.type = "button";
      b.className = "opt" + (it.v === selected ? " active" : "");
      b.textContent = it.l;
      b.addEventListener("click", function () {
        Array.prototype.forEach.call(container.children, function (c) { c.classList.remove("active"); });
        b.classList.add("active");
        onPick(it.v);
      });
      container.appendChild(b);
    });
  }

  // ── form → API mapping ──────────────────────────────────────────────────────────
  // The ONE place that turns the raw form answers into API values. Currency is decided from
  // (world, step-2 currency): RU → RUB; foreign → the step-2 currency. Nothing else computes
  // currency, so an impossible pair (e.g. RU + USD) simply can't be sent.
  function apiCurrency() { return selWorld === "RU" ? "RUB" : selForeignCurrency; }
  function formToApi() {
    return { currency: apiCurrency(), paymentMethod: selProvider, promoCode: selPromo };
  }

  // Load the promo-step price. The form is unusable until the price is in: a spinner replaces
  // the amount and «Далее» stays disabled while loading; on success the price shows and the
  // button unlocks; on failure the button STAYS disabled (can't order without a price) and the
  // error is surfaced — go back and forward to retry.
  function loadPrice() {
    var s = ++priceSeq;
    var cur = apiCurrency();
    priceLine.innerHTML = '<span class="price-spin"></span>';
    nextBtn.disabled = true;
    wizStatus.classList.remove("err"); wizStatus.textContent = "";
    api("/miniapp/price", { currency: cur }).then(function (res) {
      if (s !== priceSeq) { return; }
      priceLine.textContent = fmtMoney(res.amount, res.currency || cur, false);
      nextBtn.disabled = false;
    }).catch(function () {
      if (s !== priceSeq) { return; }
      priceLine.textContent = "—";
      nextBtn.disabled = true;
      wizStatus.classList.add("err");
      wizStatus.textContent = "Не удалось загрузить цену. Вернитесь назад и попробуйте снова.";
    });
  }

  function invoiceKey() { return [apiCurrency(), selProvider, selPromo].join("|"); }

  // Create the invoice for the current form (cached per API tuple) → Promise of
  // {payUrl, amount, currency, claimId}. Same tuple already minted → return the cached link.
  function ensureInvoice() {
    var key = invoiceKey();
    if (invoiceCache[key]) { return Promise.resolve(invoiceCache[key]); }
    var form = formToApi();
    return api("/miniapp/checkout", form).then(function (res) {
      if (res.alreadyActive) { throw { alreadyActive: true }; }
      // createdAt = mint moment (now); the invoice was created server-side a few hundred ms
      // ago — negligible vs the TTL. ttlMs comes from the server (no hardcoded lifetime here).
      var entry = { payUrl: res.payUrl, amount: res.amount, currency: res.currency || form.currency,
                    claimId: res.claimId, createdAt: Date.now(), ttlMs: res.ttlMs };
      invoiceCache[key] = entry;
      return entry;
    });
  }

  function summaryRow(k, v, cls) {
    var r = document.createElement("div"); r.className = "row" + (cls ? " " + cls : "");
    var kk = document.createElement("span"); kk.className = "k"; kk.textContent = k;
    var vv = document.createElement("span"); vv.className = "v"; vv.textContent = v;
    r.appendChild(kk); r.appendChild(vv); summaryEl.appendChild(r);
  }
  function buildFinal() {
    var inv = invoiceCache[invoiceKey()] || {};
    var cur = apiCurrency();
    summaryEl.innerHTML = "";
    summaryRow("Оплата", selWorld === "RU" ? "Российская карта" : "Иностранная карта");
    if (selPromo) { summaryRow("Промокод", selPromo); }
    summaryRow("К оплате", fmtMoney(inv.amount, inv.currency || cur, false), "total");
  }

  // Show the step at stepIdx and run its enter-logic.
  function renderStep() {
    show(wizSpinner, false); show(nav, true);
    nextBtn.disabled = false; backBtn.disabled = false;
    wizStatus.textContent = ""; wizStatus.classList.remove("err");
    // The expiry watch is only meaningful while the final checkout (with an open order) is
    // shown — stop it on every render, restart it in the final branch below.
    stopExpiryWatch();
    var steps = seq();
    var name = steps[stepIdx];
    // progress dots — filled up to and including the current step
    progress.innerHTML = "";
    for (var pi = 0; pi < steps.length; pi++) {
      var d = document.createElement("div");
      d.className = "dot" + (pi <= stepIdx ? " on" : "");
      progress.appendChild(d);
    }
    show(progress, true);
    Object.keys(stepEls).forEach(function (k) { show(stepEls[k], k === name); });
    show(backBtn, stepIdx > 0);
    nextBtn.textContent = (name === "final") ? "Оплатить" : "Далее";
    if (name === "world") {
      renderOptions(worldSel, [{ v: "RU", l: "Российская карта" }, { v: "INTL", l: "Иностранная карта" }], selWorld, function (v) {
        selWorld = v;   // raw answer only — currency is mapped later (apiCurrency), not stored here
      });
    } else if (name === "currency") {
      renderOptions(curSel, [{ v: "USD", l: "USD · $" }, { v: "EUR", l: "EUR · €" }], selForeignCurrency, function (v) { selForeignCurrency = v; });
    } else if (name === "promo") {
      // Currency is known by now → show the offer's list price above the promo field.
      loadPrice();
      promoInput.value = selPromo || promoInput.value;
    } else if (name === "final") {
      buildFinal();
      startExpiryWatch();
    }
  }

  function go(delta) { stopPolling(); priceSeq++; stepIdx += delta; if (stepIdx < 0) { stepIdx = 0; } renderStep(); }

  function writeClaimInfo() {
    var box = document.getElementById("claimInfo");
    var u = (tg && tg.initDataUnsafe && tg.initDataUnsafe.user) || {};
    // Diagnostic preview only — the real receipt address (with the global bill seq) is built
    // server-side: tg.<username|id>.<seq>@rcpt.renorma.app.
    var ident = u.username ? String(u.username).toLowerCase() : (u.id || "");
    var email = "tg." + ident + ".<seq>@rcpt.renorma.app";
    if (box && claimId) { box.textContent = "claim: " + claimId + "\n" + email; }
  }

  function onNext() {
    var name = seq()[stepIdx];
    if (name === "world") {
      if (!selWorld) { err("Выберите способ оплаты."); return; }
      go(1);
    } else if (name === "currency") {
      if (!selForeignCurrency) { err("Выберите валюту."); return; }
      go(1);
    } else if (name === "promo") {
      // Create (or reuse) the invoice for the current tuple, THEN advance to the summary.
      selPromo = promoInput.value.trim();
      busy(true, "Создаём счёт…");
      ensureInvoice().then(function () { go(1); }).catch(function (e) {
        if (e && e.alreadyActive) { showAccess("Подписка уже активна.", null); return; }
        // Show the SERVER's reason (mapped from lava), not a guess.
        err((e && e.userMessage) ? e.userMessage : "Не удалось создать счёт. Попробуйте ещё раз.");
      });
    } else if (name === "final") {
      var inv = invoiceCache[invoiceKey()];
      if (!inv || !inv.payUrl) { err("Счёт не найден — вернитесь на шаг назад."); return; }
      claimId = inv.claimId; writeClaimInfo();
      openLink(inv.payUrl);
      // STAY on the final checkout (Оплатить + Назад) — the user may come back and pick a
      // different method. Poll in the background; only a CONFIRMED payment replaces the
      // screen (showAccess). No «Ожидаем оплату» spinner here.
      wizStatus.classList.remove("err");
      wizStatus.textContent = "После оплаты доступ откроется здесь автоматически.";
      startPolling();
    }
  }

  // ── invoice-expiry watch ────────────────────────────────────────────────────────
  // The open order (lava invoice) is payable only for its TTL. Once it passes HALF its
  // lifetime the pay link is close enough to expiry that we discard the whole form and make
  // the user start over from the world step (which mints a brand-new invoice). Runs ONLY on
  // the final checkout AND while the page is visible: hidden → no checks; back to foreground
  // → an immediate check, then periodic.
  var EXPIRY_CHECK_MS = 30000;
  function orderHalfLifeExceeded() {
    var inv = invoiceCache[invoiceKey()];
    if (!inv || !inv.createdAt || !inv.ttlMs) { return false; }
    return (Date.now() - inv.createdAt) > (inv.ttlMs / 2);
  }
  function checkExpiry() {
    if (orderHalfLifeExceeded()) {
      stopExpiryWatch();
      resetForm("Счёт устарел — оформите заново.");
    }
  }
  function stopExpiryWatch() { if (expiryTimer) { clearInterval(expiryTimer); expiryTimer = null; } }
  function startExpiryWatch() {
    stopExpiryWatch();
    if (document.hidden) { return; }   // wait for visibilitychange to resume
    checkExpiry();                     // immediate check on (re)start
    expiryTimer = setInterval(checkExpiry, EXPIRY_CHECK_MS);
  }

  // Wipe every form answer + the cached invoices and return to the world step from scratch.
  function resetForm(statusText) {
    stopPolling(); stopExpiryWatch();
    invoiceCache = {};
    selWorld = null; selForeignCurrency = null; selPromo = "";
    promoInput.value = "";
    claimId = null;
    stepIdx = 0;
    renderStep();
    if (statusText) { wizStatus.classList.remove("err"); wizStatus.textContent = statusText; }
  }

  function stopPolling() { if (pollTimer) { clearInterval(pollTimer); pollTimer = null; } }
  function startPolling() {
    stopPolling();
    pollTimer = setInterval(function () {
      api("/miniapp/status", { claimId: claimId }).then(function (res) {
        if ((res.status === "paid" || res.status === "claimed") && res.onboardUrl) {
          stopPolling();
          accessUrl = res.onboardUrl;
          mintOnAccess = true;   // just paid, not onboarded → mint a fresh code on each press
          createBtn.textContent = "Получить доступ к re:Norma";
          showAccess("Оплата получена.", res.onboardUrl);
        }
      }).catch(function () { /* transient — keep waiting */ });
    }, 3000);
  }

  function showAccess(statusText, url) {
    if (url) { accessUrl = url; }
    stopPolling(); stopExpiryWatch();   // access granted — no more polling/expiry checks
    show(wizard, false); show(accessEl, true);
    accessStatus.textContent = statusText || "Подписка активна.";
  }
  function startWizard() {
    show(accessEl, false); show(wizard, true);
    stepIdx = 0; renderStep();
  }

  backBtn.addEventListener("click", function () { if (stepIdx > 0) { go(-1); } });
  nextBtn.addEventListener("click", onNext);
  createBtn.addEventListener("click", function () {
    // claimed / already onboarded → just open the app root (no code needed).
    if (!mintOnAccess) { if (accessUrl) { openLink(accessUrl); } return; }
    // paid, not onboarded → mint a BRAND-NEW code every press, then open the link with it.
    createBtn.disabled = true;
    accessStatus.classList.remove("err");
    api("/miniapp/access-link", {}).then(function (res) {
      createBtn.disabled = false;
      if (!res.onboardUrl) { accessStatus.classList.add("err"); accessStatus.textContent = "Не удалось получить ссылку — попробуйте ещё раз."; return; }
      accessUrl = res.onboardUrl;
      openLink(res.onboardUrl);
    }).catch(function () {
      createBtn.disabled = false;
      accessStatus.classList.add("err");
      accessStatus.textContent = "Не удалось получить ссылку — попробуйте ещё раз.";
    });
  });

  // Page in background → stop the expiry watch (no checks). Back to foreground → resume it
  // immediately IF the final checkout is open (it does an immediate check on start).
  document.addEventListener("visibilitychange", function () {
    if (document.hidden) { stopExpiryWatch(); }
    else { refreshAccess(); if (seq()[stepIdx] === "final") { startExpiryWatch(); } }
  });

  // Apply a /miniapp/me result to the access screen. `present=true` (initial load) shows the
  // screen; `present=false` (a foreground refresh) only updates the button + status text in
  // place. Returns false when there's no paid/claimed sub (caller shows the wizard).
  // hasAccess = the account entered the app (first chapter available) → «Открыть приложение»
  // (open the app root). Otherwise access isn't set up yet → «Получить доступ» mints a FRESH
  // code on each press.
  function applyAccess(res, present) {
    if (!res.onboardUrl) { return false; }
    var hasAccess = !!res.hasAccess;
    var url = hasAccess ? (res.appUrl || res.onboardUrl) : res.onboardUrl;
    accessUrl = url;
    mintOnAccess = !hasAccess;
    createBtn.textContent = hasAccess ? "Открыть приложение" : "Получить доступ к re:Norma";
    if (res.subStatus === "expired") { if (present) { startWizard(); } return true; }
    var days = (typeof res.daysLeft === "number") ? res.daysLeft : null;
    var cancelled = res.subStatus === "cancelled" || res.noRenew === true;
    var txt = "Подписка активна.";
    if (cancelled && days !== null) { txt = "Подписка отменена · доступ ещё " + days + " " + ruDays(days) + "."; }
    else if (days !== null) { txt = "Подписка активна · " + days + " " + ruDays(days) + "."; }
    if (present) { showAccess(txt, url); } else { accessStatus.textContent = txt; }
    return true;
  }

  // On load: already subscribed → access screen; otherwise start the wizard.
  function init() {
    show(accessEl, false); show(wizard, true);
    Object.keys(stepEls).forEach(function (k) { show(stepEls[k], false); });
    show(progress, false); show(nav, false); show(wizSpinner, true);
    wizStatus.classList.remove("err"); wizStatus.textContent = "Проверяем подписку…";
    api("/miniapp/me", {}).then(function (res) {
      if (!applyAccess(res, true)) { startWizard(); }
    }).catch(function () { startWizard(); });
  }

  // Telegram RESUMES the Mini App webview when the user returns (it does NOT reload), so a
  // stale «Получить доступ» would linger after they finished onboarding in the browser. Re-fetch
  // /miniapp/me on foreground and refresh the button/status IN PLACE. Only while the access
  // screen is up — never yanks the user out of the pay wizard.
  function refreshAccess() {
    if (accessEl.classList.contains("hidden")) { return; }
    api("/miniapp/me", {}).then(function (res) { applyAccess(res, false); }).catch(function () {});
  }
  window.addEventListener("focus", refreshAccess);

  init();
})();
</script>
</body>
</html>
"##;

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

use crate::init_data::validate_init_data;
use crate::{call_internal_checkout, do_post, error_response, session_stub, token};

// ── GET / : the Mini App page ───────────────────────────────────────────────────
pub fn serve_miniapp_page() -> Result<Response> {
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
    Ok(Response::ok(MINIAPP_HTML)?.with_headers(headers))
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
/// validated tg_user_id. The raw initData and secret_key are NEVER logged.
async fn require_init_data(
    env: &Env,
    init_data: &str,
) -> std::result::Result<i64, Response> {
    let token = match token::secret_or_var(env, "TELEGRAM_BOT_TOKEN").await {
        Ok(t) => t,
        Err(reason) => {
            console_error!("require_init_data: {reason}");
            return Err(error_response("misconfigured", 503));
        }
    };
    let now_ms = Date::now().as_millis() as i64;
    match validate_init_data(init_data, &token, now_ms) {
        Ok(ok) => Ok(ok.tg_user_id),
        Err(_reason) => {
            // Do NOT log the raw initData; the reason can reference values from it.
            Err(error_response("unauthorized", 401))
        }
    }
}

// ── POST /miniapp/checkout ───────────────────────────────────────────────────────
pub async fn miniapp_checkout(mut req: Request, env: &Env) -> Result<Response> {
    let body: serde_json::Value = req.json().await.unwrap_or(serde_json::json!({}));
    let init_data = extract_init_data(&body, &req);

    // [SEC #1] initData validation on every /miniapp/* call.
    let tg_user_id = match require_init_data(env, &init_data).await {
        Ok(id) => id,
        Err(resp) => return Ok(resp),
    };

    let promo_code = body
        .get("promoCode")
        .and_then(|v| v.as_str())
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .map(String::from);

    let plan_id = env
        .var("PLAN_ID")
        .map(|v| v.to_string())
        .unwrap_or_else(|_| "monthly".into());

    // INTERNAL_PUSH_KEY-gated on the payment side. On failure: log loudly (no secret in
    // message), 502, store NO claim.
    let checkout = match call_internal_checkout(env, &plan_id, promo_code.as_deref()).await {
        Ok(c) => c,
        Err(e) => {
            console_error!("miniapp_checkout: internal/checkout failed: {e}");
            return Ok(error_response("checkout_failed", 502));
        }
    };

    // [SEC #2] Store ownership: claimId → {tg_user_id, secret}. Secret lives only in the
    // DO, never logged.
    let stub = session_stub(env)?;
    do_post(
        &stub,
        "/miniapp/claims/put",
        &serde_json::json!({
            "claimId": checkout.claim_id,
            "tgUserId": tg_user_id,
            "secret": checkout.secret,
        }),
    )
    .await?;

    // [SEC #3] Respond with claimId + payUrl ONLY — never the secret.
    Response::from_json(&serde_json::json!({
        "claimId": checkout.claim_id,
        "payUrl": checkout.pay_url,
    }))
}

// ── POST /miniapp/status ─────────────────────────────────────────────────────────
pub async fn miniapp_status(mut req: Request, env: &Env) -> Result<Response> {
    let body: serde_json::Value = req.json().await.unwrap_or(serde_json::json!({}));
    let init_data = extract_init_data(&body, &req);

    // [SEC #1] initData validation.
    let tg_user_id = match require_init_data(env, &init_data).await {
        Ok(id) => id,
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

    // Owner lookup. Unknown claim → {status:"none"} (no secret).
    let stub = session_stub(env)?;
    let mut got = do_post(
        &stub,
        "/miniapp/claims/get",
        &serde_json::json!({ "claimId": claim_id }),
    )
    .await?;
    let cv: serde_json::Value = got.json().await?;
    if cv.get("found").and_then(|v| v.as_bool()) == Some(false) {
        return Response::from_json(&serde_json::json!({ "status": "none" }));
    }
    // [SEC #4] Owner check: one user can't read another user's claim/secret.
    let owner = cv.get("tgUserId").and_then(|v| v.as_i64());
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

    // [SEC #5] Release the secret ONLY on paid/claimed, ONLY to the validated owner.
    if status == "paid" || status == "claimed" {
        let base = env
            .var("APP_ONBOARD_URL")
            .map(|v| v.to_string())
            .unwrap_or_else(|_| "https://fit.renorma.app/onboard".into());
        // FRAGMENT (#claim=...) — the secret is NEVER logged.
        let onboard_url = format!("{base}#claim={claim_id}.{secret}");
        Response::from_json(&serde_json::json!({
            "status": status,
            "onboardUrl": onboard_url,
        }))
    } else {
        // pending / void / none → no secret, no onboardUrl.
        Response::from_json(&serde_json::json!({ "status": status }))
    }
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
  :root { color-scheme: light dark; }
  * { box-sizing: border-box; }
  body {
    margin: 0;
    font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, sans-serif;
    background: var(--tg-theme-bg-color, #ffffff);
    color: var(--tg-theme-text-color, #111111);
    min-height: 100vh;
    display: flex;
    align-items: center;
    justify-content: center;
    padding: 20px;
  }
  .card {
    width: 100%;
    max-width: 420px;
    display: flex;
    flex-direction: column;
    gap: 16px;
  }
  h1 { font-size: 20px; margin: 0 0 4px; }
  p.sub { margin: 0; font-size: 14px; color: var(--tg-theme-hint-color, #888); }
  input {
    width: 100%;
    padding: 14px;
    font-size: 16px;
    border-radius: 12px;
    border: 1px solid var(--tg-theme-hint-color, #ccc);
    background: var(--tg-theme-secondary-bg-color, #f4f4f5);
    color: var(--tg-theme-text-color, #111);
  }
  button {
    width: 100%;
    padding: 15px;
    font-size: 16px;
    font-weight: 600;
    border: none;
    border-radius: 12px;
    background: var(--tg-theme-button-color, #2ea6ff);
    color: var(--tg-theme-button-text-color, #ffffff);
    cursor: pointer;
  }
  button:disabled { opacity: 0.6; cursor: default; }
  .status { text-align: center; font-size: 15px; min-height: 22px; }
  .status.err { color: #e53935; }
  .spinner {
    width: 28px; height: 28px; margin: 8px auto 0;
    border: 3px solid var(--tg-theme-hint-color, #ccc);
    border-top-color: var(--tg-theme-button-color, #2ea6ff);
    border-radius: 50%;
    animation: spin 0.8s linear infinite;
  }
  @keyframes spin { to { transform: rotate(360deg); } }
  .hidden { display: none; }
</style>
</head>
<body>
<div class="card">
  <div>
    <h1>Оформление подписки</h1>
    <p class="sub">Введите промокод, если он у вас есть, и нажмите «Оплатить».</p>
  </div>

  <input id="promo" type="text" autocomplete="off" autocapitalize="off"
         placeholder="Промокод (необязательно)">

  <button id="payBtn">Оплатить</button>
  <button id="createBtn" class="hidden">Создать аккаунт</button>

  <div id="status" class="status"></div>
  <div id="spinner" class="spinner hidden"></div>
</div>

<script>
(function () {
  var tg = window.Telegram && window.Telegram.WebApp;
  if (tg) { tg.ready(); tg.expand(); }
  var initData = tg ? tg.initData : "";

  var promoInput = document.getElementById("promo");
  var payBtn = document.getElementById("payBtn");
  var createBtn = document.getElementById("createBtn");
  var statusEl = document.getElementById("status");
  var spinnerEl = document.getElementById("spinner");

  var claimId = null;
  var onboardUrl = null;
  var pollTimer = null;

  function show(el, on) { el.classList.toggle("hidden", !on); }

  function setState(state) {
    statusEl.classList.remove("err");
    if (state === "idle") {
      show(payBtn, true); payBtn.disabled = false; payBtn.textContent = "Оплатить";
      show(createBtn, false); show(spinnerEl, false);
      promoInput.disabled = false;
      statusEl.textContent = "";
    } else if (state === "creating") {
      show(payBtn, true); payBtn.disabled = true; payBtn.textContent = "Создаём оплату…";
      show(createBtn, false); show(spinnerEl, false);
      promoInput.disabled = true;
      statusEl.textContent = "";
    } else if (state === "awaiting") {
      show(payBtn, false); show(createBtn, false); show(spinnerEl, true);
      promoInput.disabled = true;
      statusEl.textContent = "Ожидаем оплату…";
    } else if (state === "paid") {
      show(payBtn, false); show(spinnerEl, false);
      show(createBtn, true); createBtn.disabled = false;
      statusEl.textContent = "Оплата получена.";
    } else if (state === "error") {
      show(spinnerEl, false); show(createBtn, false);
      show(payBtn, true); payBtn.disabled = false; payBtn.textContent = "Повторить";
      promoInput.disabled = false;
      statusEl.classList.add("err");
      statusEl.textContent = "Что-то пошло не так. Попробуйте ещё раз.";
    }
  }

  function api(path, body) {
    body = body || {};
    body.initData = initData;
    return fetch(path, {
      method: "POST",
      headers: { "Content-Type": "application/json", "X-Telegram-Init-Data": initData },
      body: JSON.stringify(body)
    }).then(function (r) {
      if (!r.ok) { throw new Error("http " + r.status); }
      return r.json();
    });
  }

  function openLink(url) {
    if (tg && typeof tg.openLink === "function") { tg.openLink(url); }
    else { window.open(url, "_blank"); }
  }

  function stopPolling() {
    if (pollTimer) { clearInterval(pollTimer); pollTimer = null; }
  }

  function startPolling() {
    stopPolling();
    pollTimer = setInterval(function () {
      api("/miniapp/status", { claimId: claimId }).then(function (res) {
        if ((res.status === "paid" || res.status === "claimed") && res.onboardUrl) {
          stopPolling();
          onboardUrl = res.onboardUrl;
          setState("paid");
        }
        // pending / void / none → keep waiting.
      }).catch(function () {
        // Transient network error: keep polling, do not drop the awaiting state.
      });
    }, 3000);
  }

  function onPay() {
    setState("creating");
    var promoCode = promoInput.value.trim();
    api("/miniapp/checkout", { promoCode: promoCode }).then(function (res) {
      claimId = res.claimId;
      openLink(res.payUrl);
      setState("awaiting");
      startPolling();
    }).catch(function () {
      setState("error");
    });
  }

  payBtn.addEventListener("click", onPay);
  createBtn.addEventListener("click", function () {
    if (onboardUrl) { openLink(onboardUrl); }
  });

  setState("idle");
})();
</script>
</body>
</html>
"##;

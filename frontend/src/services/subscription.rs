use serde::{Deserialize, Serialize};
use wasm_bindgen::JsCast;
use wasm_bindgen::JsValue;
use wasm_bindgen_futures::JsFuture;

use super::{app_flags, auth, config};

const LS_KEY: &str = "ft_subscription";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Status {
    pub plan: String,
    pub end: i64,
    pub active: bool,
    // Extra fields for the Settings "manage subscription" view. `serde(default)`
    // so older cached entries / the gate-only reads still deserialize.
    #[serde(default)]
    pub start: i64, // ms epoch the subscription/trial began
    #[serde(default)]
    pub status: Option<String>, // "paid" | "cancelled" | "expired"
    #[serde(default)]
    pub no_renew: Option<bool>,
    #[serde(default)]
    pub provider: Option<String>,
}

impl Status {
    pub fn is_paid(&self) -> bool {
        self.status.as_deref() == Some("paid") && self.active
    }
}

/// A purchasable plan (from the payment-worker catalog; offer ids stay server-side).
#[derive(Debug, Clone, Deserialize)]
pub struct Plan {
    pub id: String,
    pub title: String,
    pub price: f64,
    pub currency: String,
    pub period: String,
}

/// Last-known subscription status, cached in localStorage. Lets the Story page
/// gate chapter 2 while briefly offline; refreshed on every successful fetch.
pub fn cached() -> Option<Status> {
    let json = app_flags::get(LS_KEY)?;
    serde_json::from_str(&json).ok()
}

fn cache(status: &Status) {
    let Ok(json) = serde_json::to_string(status) else { return };
    app_flags::set(LS_KEY, &json);
}

pub async fn status() -> Result<Status, String> {
    let s: Status = request("GET", "/subscription", None).await?;
    cache(&s);
    Ok(s)
}

/// Available subscription plans for the paywall.
pub async fn plans() -> Result<Vec<Plan>, String> {
    #[derive(Deserialize)]
    struct Resp {
        plans: Vec<Plan>,
    }
    let r: Resp = request("GET", "/plans", None).await?;
    Ok(r.plans)
}

/// Claim a paid guest subscription (post-registration). Binds it to this account.
///
/// The `claim_id`/`secret` come from the `#claim=claimId.secret` fragment lava
/// redirected to after payment. The server does an atomic compare-and-set inside
/// the ClaimDO: idempotent for the same account, hard-rejected (403) for another.
/// Success requires the signed webhook to have already marked the record paid; a
/// not-yet-paid claim returns HTTP 409 `not_paid_yet` (the onboarding retries).
pub async fn claim(claim_id: &str, secret: &str) -> Result<Status, String> {
    let body = serde_json::json!({ "claimId": claim_id, "secret": secret });
    let s: Status = request("POST", "/claim", Some(body)).await?;
    cache(&s);
    Ok(s)
}

/// Test-only entitlement path used by e2e: mints a deterministically-paid guest
/// claim WITHOUT taking real money. Reachable only when the worker has
/// `TEST_ENTITLEMENT=1` (absent in production, where `/test/*` returns 404), so
/// compiling this in always is harmless in prod. Returns `(claim_id, secret)`.
pub async fn test_guest_checkout(plan_id: &str) -> Result<(String, String), String> {
    #[derive(Deserialize)]
    struct Resp {
        #[serde(rename = "claimId")]
        claim_id: String,
        secret: String,
    }
    let body = serde_json::json!({ "planId": plan_id });
    // Unauthenticated (the user doesn't exist yet) — use the dedicated helper.
    let r: Resp = request_unauthed("POST", "/test/guest-checkout", Some(body)).await?;
    Ok((r.claim_id, r.secret))
}

/// Cancel auto-renew (stays active until the period ends).
pub async fn cancel() -> Result<Status, String> {
    let s: Status = request("POST", "/cancel", None).await?;
    cache(&s);
    Ok(s)
}

/// The prorated refund the user would get (server-computed; no side effects).
#[derive(Debug, Clone, Deserialize)]
pub struct RefundPreview {
    pub amount: i64,
    #[serde(default)]
    pub currency: String,
    #[serde(rename = "daysLeft", default)]
    pub days_left: i64,
}

/// Preview the refund amount without touching anything.
pub async fn refund_preview() -> Result<RefundPreview, String> {
    request("POST", "/refund/preview", None).await
}

/// Request the refund: records it for the operator AND revokes access immediately.
/// Refetches the (now-revoked) status into the cache.
pub async fn refund_request() -> Result<Status, String> {
    let _: serde_json::Value = request("POST", "/refund/request", None).await?;
    let s = status().await?;
    Ok(s)
}

async fn request<T: serde::de::DeserializeOwned>(
    method: &str,
    path: &str,
    body: Option<serde_json::Value>,
) -> Result<T, String> {
    let token = auth::get_token().ok_or_else(|| "not authenticated".to_string())?;
    request_inner(method, path, body, Some(&token)).await
}

/// Like [`request`] but sends no `Authorization` header. Used for the
/// unauthenticated test-entitlement path (the user doesn't exist yet).
async fn request_unauthed<T: serde::de::DeserializeOwned>(
    method: &str,
    path: &str,
    body: Option<serde_json::Value>,
) -> Result<T, String> {
    request_inner(method, path, body, None).await
}

async fn request_inner<T: serde::de::DeserializeOwned>(
    method: &str,
    path: &str,
    body: Option<serde_json::Value>,
    token: Option<&str>,
) -> Result<T, String> {
    let base = &config::get().payment_base_url;
    let url = format!("{base}{path}");

    let opts = web_sys::RequestInit::new();
    opts.set_method(method);
    if let Some(b) = body {
        let body_str = serde_json::to_string(&b).map_err(|e| e.to_string())?;
        opts.set_body(&JsValue::from_str(&body_str));
    }

    let headers = web_sys::Headers::new().map_err(|e| format!("{e:?}"))?;
    headers.set("Content-Type", "application/json").map_err(|e| format!("{e:?}"))?;
    if let Some(token) = token {
        headers.set("Authorization", &format!("Bearer {token}")).map_err(|e| format!("{e:?}"))?;
    }
    opts.set_headers(&headers);

    let request = web_sys::Request::new_with_str_and_init(&url, &opts)
        .map_err(|e| format!("{e:?}"))?;
    let window = web_sys::window().expect("no window");
    let resp_val = JsFuture::from(window.fetch_with_request(&request))
        .await
        .map_err(|e| format!("{e:?}"))?;
    let resp: web_sys::Response = resp_val.dyn_into().map_err(|_| "not a Response".to_string())?;

    let text = JsFuture::from(resp.text().map_err(|e| format!("{e:?}"))?)
        .await
        .map_err(|e| format!("{e:?}"))?;
    let text = text.as_string().ok_or("response not string")?;

    if !resp.ok() {
        return Err(format!("HTTP {}: {}", resp.status(), text));
    }
    serde_json::from_str(&text).map_err(|e| format!("parse error: {e}"))
}

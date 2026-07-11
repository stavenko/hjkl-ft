use std::cell::RefCell;

use leptos::*;
use serde::{Deserialize, Serialize};
use wasm_bindgen::JsCast;
use wasm_bindgen::JsValue;
use wasm_bindgen_futures::JsFuture;

use super::{app_flags, auth, config};

const LS_KEY: &str = "ft_subscription";
/// When the cached status was last confirmed against the server (ms epoch).
const CHECKED_AT_KEY: &str = "ft_subscription_checked_at";
/// Re-verify the subscription at most once per day, on connectivity.
const RECHECK_MS: f64 = 24.0 * 60.0 * 60.0 * 1000.0;

thread_local! {
    // Reactive gate the App watches: true = active subscription. Seeded from the
    // cache at init so a returning user enters instantly (offline-first); a daily
    // re-check flips it, which drops the App into `Locked` (or back to `Ready`).
    // Created at the ROOT via init(), like `update`/`net` signals.
    static GATE: RefCell<Option<RwSignal<bool>>> = const { RefCell::new(None) };
}

/// Create the reactive subscription gate in the root scope, seeded from the
/// cached status. Call once from main() before mounting.
pub fn init() {
    GATE.with(|c| {
        if c.borrow().is_none() {
            let active = cached().map(|s| s.active).unwrap_or(false);
            *c.borrow_mut() = Some(create_rw_signal(active));
        }
    });
}

/// Reactive gate: true when the session holds an active subscription. The App
/// effect observes this to lock/unlock the interface when the status changes.
pub fn gate_signal() -> RwSignal<bool> {
    GATE.with(|c| c.borrow().expect("subscription::init() must run before gate_signal()"))
}

fn set_gate(active: bool) {
    GATE.with(|c| {
        if let Some(sig) = *c.borrow() {
            sig.set(active);
        }
    });
}

fn now_ms() -> f64 {
    js_sys::Date::now()
}

/// When the cached status was last confirmed against the server, if ever.
fn last_checked_at() -> Option<f64> {
    app_flags::get(CHECKED_AT_KEY)?.parse::<f64>().ok()
}

/// True when we have never verified, or the last check is older than a day.
/// Callers additionally require connectivity + a session.
pub fn recheck_due() -> bool {
    match last_checked_at() {
        Some(t) => now_ms() - t >= RECHECK_MS,
        None => true,
    }
}

/// Re-verify the subscription against the server IF a session exists and the last
/// check is a day old. Called on each "came online" event (see `net`/bootstrap).
/// `status()` rewrites the cache + stamp + gate, so a changed status
/// automatically locks/unlocks the App via [`gate_signal`]. FAIL-loud on error
/// (logged) — a transient failure leaves the last-known status in place.
pub async fn maybe_recheck() {
    if auth::get_token().is_none() || !recheck_due() {
        return;
    }
    if let Err(e) = status().await {
        leptos::logging::warn!("subscription recheck failed: {e}");
    }
}

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

/// Last-known subscription status, cached in localStorage. Lets the Story page
/// gate chapter 2 while briefly offline; refreshed on every successful fetch.
pub fn cached() -> Option<Status> {
    let json = app_flags::get(LS_KEY)?;
    serde_json::from_str(&json).ok()
}

fn cache(status: &Status) {
    let Ok(json) = serde_json::to_string(status) else { return };
    app_flags::set(LS_KEY, &json);
    // Stamp the verification time and update the reactive gate so a changed
    // status flips the App between Ready and Locked.
    app_flags::set(CHECKED_AT_KEY, &now_ms().to_string());
    set_gate(status.active);
}

pub async fn status() -> Result<Status, String> {
    let s: Status = request("GET", "/subscription", None).await?;
    cache(&s);
    Ok(s)
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

/// Claim lifecycle status (public, no auth): `none | pending | paid | claimed | void`.
/// Used to PRE-CHECK before registering (F-1): if the claim can't be bound (terminal
/// `claimed`/`void`/`none`), we don't create an account, so no orphan account is left.
pub async fn claim_status(claim_id: &str) -> Result<String, String> {
    #[derive(Deserialize)]
    struct Resp {
        status: String,
    }
    // claim_id is URL-safe base64url — no encoding needed.
    let r: Resp = request_unauthed("GET", &format!("/claim/status?claimId={claim_id}"), None).await?;
    Ok(r.status)
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
    // A fetch rejection means the payment worker is unreachable — drop its flag
    // immediately (surfaces the degraded warning before the next probe).
    let resp_val = match JsFuture::from(window.fetch_with_request(&request)).await {
        Ok(v) => v,
        Err(e) => {
            super::net::note_failure(super::net::Worker::Payment);
            return Err(format!("{e:?}"));
        }
    };
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

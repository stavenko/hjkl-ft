use serde::{Deserialize, Serialize};
use wasm_bindgen::JsCast;
use wasm_bindgen::JsValue;
use wasm_bindgen_futures::JsFuture;

use super::{auth, config};

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
    pub status: Option<String>, // "trial" | "paid" | "cancelled" | "expired"
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
    let storage = web_sys::window()?.local_storage().ok()??;
    let json = storage.get_item(LS_KEY).ok()??;
    serde_json::from_str(&json).ok()
}

fn cache(status: &Status) {
    let Ok(json) = serde_json::to_string(status) else { return };
    if let Some(Ok(Some(storage))) = web_sys::window().map(|w| w.local_storage()) {
        let _ = storage.set_item(LS_KEY, &json);
    }
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

/// Start checkout for a plan via a provider; returns the hosted-checkout URL to
/// redirect the browser to. (Caller does `window.location().set_href(url)`.)
pub async fn checkout(provider: &str, plan_id: &str) -> Result<String, String> {
    #[derive(Deserialize)]
    struct Resp {
        url: String,
    }
    let body = serde_json::json!({ "provider": provider, "planId": plan_id });
    let r: Resp = request("POST", "/checkout", Some(body)).await?;
    Ok(r.url)
}

/// Cancel auto-renew (stays active until the period ends).
pub async fn cancel() -> Result<Status, String> {
    let s: Status = request("POST", "/cancel", None).await?;
    cache(&s);
    Ok(s)
}

async fn request<T: serde::de::DeserializeOwned>(
    method: &str,
    path: &str,
    body: Option<serde_json::Value>,
) -> Result<T, String> {
    let base = &config::get().payment_base_url;
    let url = format!("{base}{path}");
    let token = auth::get_token().ok_or_else(|| "not authenticated".to_string())?;

    let opts = web_sys::RequestInit::new();
    opts.set_method(method);
    if let Some(b) = body {
        let body_str = serde_json::to_string(&b).map_err(|e| e.to_string())?;
        opts.set_body(&JsValue::from_str(&body_str));
    }

    let headers = web_sys::Headers::new().map_err(|e| format!("{e:?}"))?;
    headers.set("Content-Type", "application/json").map_err(|e| format!("{e:?}"))?;
    headers.set("Authorization", &format!("Bearer {token}")).map_err(|e| format!("{e:?}"))?;
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

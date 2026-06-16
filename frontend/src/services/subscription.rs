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
}

impl Status {
    pub fn is_paid(&self) -> bool {
        self.plan == "paid" && self.active
    }
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
    let s = request("GET", "/subscription", None).await?;
    cache(&s);
    Ok(s)
}

pub async fn pay(code_word: &str) -> Result<Status, String> {
    let body = serde_json::json!({ "code_word": code_word });
    let s = request("POST", "/pay", Some(body)).await?;
    cache(&s);
    Ok(s)
}

async fn request(
    method: &str,
    path: &str,
    body: Option<serde_json::Value>,
) -> Result<Status, String> {
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

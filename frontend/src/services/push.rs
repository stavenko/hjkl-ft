use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use wasm_bindgen_futures::JsFuture;

const KEY_PUSH_SUBSCRIBED: &str = "push_subscribed";
const KEY_PUSH_ONBOARDING_DISMISSED: &str = "push_onboarding_dismissed";

fn window() -> web_sys::Window {
    web_sys::window().expect("no window")
}

fn storage() -> web_sys::Storage {
    window()
        .local_storage()
        .ok()
        .flatten()
        .expect("no localStorage")
}

/// Check whether the Push API is available in this browser.
pub fn is_supported() -> bool {
    let nav = window().navigator();
    let sw = js_sys::Reflect::get(&nav, &"serviceWorker".into());
    match sw {
        Ok(val) => !val.is_undefined() && !val.is_null(),
        Err(_) => false,
    }
}

/// Check localStorage flag indicating an active subscription.
pub fn is_subscribed() -> bool {
    storage()
        .get_item(KEY_PUSH_SUBSCRIBED)
        .ok()
        .flatten()
        .map(|v| v == "true")
        .unwrap_or(false)
}

fn set_subscribed(val: bool) {
    if val {
        storage()
            .set_item(KEY_PUSH_SUBSCRIBED, "true")
            .expect("write push_subscribed");
    } else {
        let _ = storage().remove_item(KEY_PUSH_SUBSCRIBED);
    }
}

pub fn needs_push_onboarding() -> bool {
    is_supported() && !is_subscribed() && !onboarding_dismissed()
}

fn onboarding_dismissed() -> bool {
    storage()
        .get_item(KEY_PUSH_ONBOARDING_DISMISSED)
        .ok()
        .flatten()
        .is_some()
}

pub fn dismiss_onboarding() {
    storage()
        .set_item(KEY_PUSH_ONBOARDING_DISMISSED, "true")
        .expect("write push_onboarding_dismissed");
}

/// Request notification permission. Returns `true` if granted.
pub async fn request_permission() -> Result<bool, String> {
    let promise = web_sys::Notification::request_permission()
        .map_err(|e| format!("{:?}", e))?;
    let result = JsFuture::from(promise)
        .await
        .map_err(|e| format!("{:?}", e))?;
    let perm = result
        .as_string()
        .unwrap_or_default();
    Ok(perm == "granted")
}

/// Fetch the VAPID public key from the server, then subscribe via PushManager
/// and POST the subscription to the backend.
pub async fn subscribe() -> Result<(), String> {
    if !is_supported() {
        return Err("Push not supported".to_string());
    }

    let granted = request_permission().await?;
    if !granted {
        return Err("Notification permission denied".to_string());
    }

    let push_base = {
        let cfg = crate::services::config::get();
        if cfg.push_base_url.is_empty() {
            if cfg.auth_base_url.is_empty() {
                cfg.api_base_url.clone()
            } else {
                cfg.auth_base_url.clone()
            }
        } else {
            cfg.push_base_url.clone()
        }
    };

    // 1. Fetch VAPID public key
    let vapid_key = fetch_vapid_key(&push_base).await?;

    // 2. Get service worker registration
    let registration = get_sw_registration().await?;

    // 3. Subscribe via PushManager
    let subscription = push_manager_subscribe(&registration, &vapid_key).await?;

    // 4. Extract subscription JSON and POST to server
    let sub_json = subscription_to_json(&subscription)?;
    post_subscription(&push_base, &sub_json).await?;

    set_subscribed(true);
    dismiss_onboarding();
    Ok(())
}

/// Unsubscribe from push notifications.
pub async fn unsubscribe() -> Result<(), String> {
    if !is_supported() {
        set_subscribed(false);
        return Ok(());
    }

    let registration = get_sw_registration().await?;
    let push_manager = js_sys::Reflect::get(&registration, &"pushManager".into())
        .map_err(|e| format!("{:?}", e))?;
    let get_sub_promise = js_sys::Reflect::apply(
        &js_sys::Reflect::get(&push_manager, &"getSubscription".into())
            .map_err(|e| format!("{:?}", e))?
            .dyn_into::<js_sys::Function>()
            .map_err(|_| "getSubscription is not a function".to_string())?,
        &push_manager,
        &js_sys::Array::new(),
    )
    .map_err(|e| format!("{:?}", e))?;

    let sub_val = JsFuture::from(js_sys::Promise::from(get_sub_promise))
        .await
        .map_err(|e| format!("{:?}", e))?;

    if !sub_val.is_null() && !sub_val.is_undefined() {
        let unsub_fn = js_sys::Reflect::get(&sub_val, &"unsubscribe".into())
            .map_err(|e| format!("{:?}", e))?
            .dyn_into::<js_sys::Function>()
            .map_err(|_| "unsubscribe is not a function".to_string())?;
        let unsub_promise = js_sys::Reflect::apply(&unsub_fn, &sub_val, &js_sys::Array::new())
            .map_err(|e| format!("{:?}", e))?;
        let _ = JsFuture::from(js_sys::Promise::from(unsub_promise))
            .await
            .map_err(|e| format!("{:?}", e))?;
    }

    set_subscribed(false);
    Ok(())
}

pub async fn sync_notification_schedule(schedule: serde_json::Value) -> Result<(), String> {
    let push_base = {
        let cfg = crate::services::config::get();
        if cfg.push_base_url.is_empty() {
            if cfg.auth_base_url.is_empty() {
                cfg.api_base_url.clone()
            } else {
                cfg.auth_base_url.clone()
            }
        } else {
            cfg.push_base_url.clone()
        }
    };

    let token = match crate::services::auth::get_token() {
        Some(t) => t,
        None => return Ok(()),
    };

    let url = format!("{}/schedule", push_base);
    let body_str = serde_json::to_string(&schedule).map_err(|e| e.to_string())?;

    let opts = web_sys::RequestInit::new();
    opts.set_method("POST");
    opts.set_body(&JsValue::from_str(&body_str));

    let headers = web_sys::Headers::new().map_err(|e| format!("{:?}", e))?;
    headers
        .set("Content-Type", "application/json")
        .map_err(|e| format!("{:?}", e))?;
    headers
        .set("Authorization", &format!("Bearer {}", token))
        .map_err(|e| format!("{:?}", e))?;
    opts.set_headers(&headers);

    let request = web_sys::Request::new_with_str_and_init(&url, &opts)
        .map_err(|e| format!("{:?}", e))?;

    let resp_val = JsFuture::from(window().fetch_with_request(&request))
        .await
        .map_err(|e| format!("{:?}", e))?;
    let resp: web_sys::Response = resp_val.dyn_into().map_err(|_| "not a Response".to_string())?;

    if !resp.ok() {
        let text = JsFuture::from(resp.text().map_err(|e| format!("{:?}", e))?)
            .await
            .map_err(|e| format!("{:?}", e))?;
        let text = text.as_string().unwrap_or_default();
        return Err(format!("HTTP {}: {}", resp.status(), text));
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

async fn fetch_vapid_key(base: &str) -> Result<String, String> {
    let url = format!("{}/push/vapid-key", base);
    let opts = web_sys::RequestInit::new();
    opts.set_method("GET");

    let request = web_sys::Request::new_with_str_and_init(&url, &opts)
        .map_err(|e| format!("{:?}", e))?;

    let resp_val = JsFuture::from(window().fetch_with_request(&request))
        .await
        .map_err(|e| format!("{:?}", e))?;
    let resp: web_sys::Response = resp_val.dyn_into().map_err(|_| "not a Response".to_string())?;

    if !resp.ok() {
        return Err(format!("HTTP {} fetching VAPID key", resp.status()));
    }

    let text = JsFuture::from(resp.text().map_err(|e| format!("{:?}", e))?)
        .await
        .map_err(|e| format!("{:?}", e))?;
    let text = text.as_string().ok_or("response not string")?;

    let parsed: serde_json::Value =
        serde_json::from_str(&text).map_err(|e| format!("parse vapid response: {}", e))?;
    parsed
        .get("public_key")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .ok_or_else(|| "missing public_key in vapid response".to_string())
}

async fn get_sw_registration() -> Result<JsValue, String> {
    let nav = window().navigator();
    let sw_container = js_sys::Reflect::get(&nav, &"serviceWorker".into())
        .map_err(|e| format!("{:?}", e))?;
    let ready = js_sys::Reflect::get(&sw_container, &"ready".into())
        .map_err(|e| format!("{:?}", e))?;
    let registration = JsFuture::from(js_sys::Promise::from(ready))
        .await
        .map_err(|e| format!("{:?}", e))?;
    Ok(registration)
}

/// Convert a base64url-encoded VAPID key to a Uint8Array for applicationServerKey.
fn vapid_to_uint8array(b64: &str) -> js_sys::Uint8Array {
    use base64::Engine;
    let bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(b64)
        .or_else(|_| base64::engine::general_purpose::STANDARD.decode(b64))
        .unwrap_or_default();
    let arr = js_sys::Uint8Array::new_with_length(bytes.len() as u32);
    arr.copy_from(&bytes);
    arr
}

async fn push_manager_subscribe(
    registration: &JsValue,
    vapid_key: &str,
) -> Result<JsValue, String> {
    let push_manager = js_sys::Reflect::get(registration, &"pushManager".into())
        .map_err(|e| format!("{:?}", e))?;

    let options = js_sys::Object::new();
    let _ = js_sys::Reflect::set(
        &options,
        &"userVisibleOnly".into(),
        &JsValue::from_bool(true),
    );
    let app_key = vapid_to_uint8array(vapid_key);
    let _ = js_sys::Reflect::set(
        &options,
        &"applicationServerKey".into(),
        &app_key.buffer(),
    );

    let subscribe_fn = js_sys::Reflect::get(&push_manager, &"subscribe".into())
        .map_err(|e| format!("{:?}", e))?
        .dyn_into::<js_sys::Function>()
        .map_err(|_| "subscribe is not a function".to_string())?;

    let args = js_sys::Array::new();
    args.push(&options);

    let promise = js_sys::Reflect::apply(&subscribe_fn, &push_manager, &args)
        .map_err(|e| format!("{:?}", e))?;

    let subscription = JsFuture::from(js_sys::Promise::from(promise))
        .await
        .map_err(|e| format!("PushManager.subscribe failed: {:?}", e))?;

    Ok(subscription)
}

fn subscription_to_json(subscription: &JsValue) -> Result<serde_json::Value, String> {
    let to_json_fn = js_sys::Reflect::get(subscription, &"toJSON".into())
        .map_err(|e| format!("{:?}", e))?
        .dyn_into::<js_sys::Function>()
        .map_err(|_| "toJSON is not a function".to_string())?;

    let json_obj = js_sys::Reflect::apply(&to_json_fn, subscription, &js_sys::Array::new())
        .map_err(|e| format!("{:?}", e))?;

    let endpoint = js_sys::Reflect::get(&json_obj, &"endpoint".into())
        .ok()
        .and_then(|v| v.as_string())
        .unwrap_or_default();

    let keys = js_sys::Reflect::get(&json_obj, &"keys".into())
        .map_err(|e| format!("{:?}", e))?;
    let p256dh = js_sys::Reflect::get(&keys, &"p256dh".into())
        .ok()
        .and_then(|v| v.as_string())
        .unwrap_or_default();
    let auth = js_sys::Reflect::get(&keys, &"auth".into())
        .ok()
        .and_then(|v| v.as_string())
        .unwrap_or_default();

    Ok(serde_json::json!({
        "endpoint": endpoint,
        "keys": {
            "p256dh": p256dh,
            "auth": auth
        }
    }))
}

async fn post_subscription(base: &str, sub_json: &serde_json::Value) -> Result<(), String> {
    let token = crate::services::auth::get_token()
        .ok_or_else(|| "not authenticated".to_string())?;
    let url = format!("{}/push/subscribe", base);
    let body_str = serde_json::to_string(sub_json).map_err(|e| e.to_string())?;

    let opts = web_sys::RequestInit::new();
    opts.set_method("POST");
    opts.set_body(&JsValue::from_str(&body_str));

    let headers = web_sys::Headers::new().map_err(|e| format!("{:?}", e))?;
    headers
        .set("Content-Type", "application/json")
        .map_err(|e| format!("{:?}", e))?;
    headers
        .set("Authorization", &format!("Bearer {}", token))
        .map_err(|e| format!("{:?}", e))?;
    opts.set_headers(&headers);

    let request = web_sys::Request::new_with_str_and_init(&url, &opts)
        .map_err(|e| format!("{:?}", e))?;

    let resp_val = JsFuture::from(window().fetch_with_request(&request))
        .await
        .map_err(|e| format!("{:?}", e))?;
    let resp: web_sys::Response = resp_val.dyn_into().map_err(|_| "not a Response".to_string())?;

    if !resp.ok() {
        let text = JsFuture::from(resp.text().map_err(|e| format!("{:?}", e))?)
            .await
            .map_err(|e| format!("{:?}", e))?;
        let text = text.as_string().unwrap_or_default();
        return Err(format!("HTTP {}: {}", resp.status(), text));
    }

    Ok(())
}

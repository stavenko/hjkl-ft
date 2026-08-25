//! Пуш куратору о новом сообщении клиента.
//!
//! Тот же механизм, что у худеющего: VAPID-ключ и подписки держит main-flow, и
//! подписка кладётся под `sub` куратора — паскей он заводил на своём домене,
//! значит и `sub` у него свой.
//!
//! Отдельный сервис-воркер здесь нужен не ради кэша, а потому что пуш принимает
//! именно он: приложение может быть закрыто.

use wasm_bindgen::{JsCast, JsValue};
use wasm_bindgen_futures::JsFuture;

use crate::{auth, config};

const KEY_SUBSCRIBED: &str = "curator_push_subscribed";

fn window() -> web_sys::Window {
    web_sys::window().expect("no window")
}

/// Есть ли на этом устройстве всё нужное для веб-пуша: сервис-воркер и глобали
/// `Notification` и `PushManager`. На iOS Safari они существуют ТОЛЬКО в
/// установленном приложении, а не во вкладке, — без этой проверки вызов
/// `Notification.requestPermission()` там падает с «Can't find variable».
pub fn is_supported() -> bool {
    let win = window();
    let present = |obj: &wasm_bindgen::JsValue, key: &str| {
        js_sys::Reflect::get(obj, &wasm_bindgen::JsValue::from_str(key))
            .map(|v| !v.is_undefined() && !v.is_null())
            .unwrap_or(false)
    };
    present(win.navigator().as_ref(), "serviceWorker")
        && present(win.as_ref(), "Notification")
        && present(win.as_ref(), "PushManager")
}

/// Подписан ли куратор на этом устройстве.
pub fn is_subscribed() -> bool {
    web_sys::window()
        .and_then(|w| w.local_storage().ok().flatten())
        .and_then(|s| s.get_item(KEY_SUBSCRIBED).ok().flatten())
        .as_deref()
        == Some("1")
}

fn set_subscribed(val: bool) {
    if let Some(s) = web_sys::window().and_then(|w| w.local_storage().ok().flatten()) {
        let _ = if val {
            s.set_item(KEY_SUBSCRIBED, "1")
        } else {
            s.remove_item(KEY_SUBSCRIBED)
        };
    }
}

/// Спросить разрешение на уведомления. `true` — дали.
pub async fn request_permission() -> Result<bool, String> {
    // Оговорка: `Notification` может отсутствовать (iOS Safari вне установленного
    // приложения) — обращение к web_sys::Notification там бросает ReferenceError.
    if !is_supported() {
        return Err("notifications_unsupported".to_string());
    }
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


/// Подписаться на уведомления. Зовётся после входа: разрешение спрашивается
/// один раз, и отказ — не ошибка, а решение человека.
pub async fn subscribe() -> Result<(), String> {
    if !is_supported() {
        return Err("push не поддерживается".to_string());
    }
    if !request_permission().await? {
        return Err("уведомления запрещены".to_string());
    }
    let base = config::get().push_base_url.clone();
    if base.is_empty() {
        return Err("push_base_url не настроен".to_string());
    }
    let vapid_key = fetch_vapid_key(&base).await?;
    let registration = get_sw_registration().await?;
    let subscription = push_manager_subscribe(&registration, &vapid_key).await?;
    let sub_json = subscription_to_json(&subscription)?;
    post_subscription(&base, &sub_json).await?;
    set_subscribed(true);
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
    let token = auth::get_token()
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

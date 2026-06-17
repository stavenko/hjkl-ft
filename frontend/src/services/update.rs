//! In-app auto-update: detect a new deploy and reload to it.
//!
//! `init.js` stamps the build id into `globalThis.__APP_VERSION__`, and the same
//! id is published at `/version.json`. On resume we poll that endpoint (cache-
//! busted; the service worker also bypasses caching it) and reload when it
//! differs. This covers the iOS-PWA-resumed-from-background case, where the app
//! is restored from memory without a navigation, so a stale build keeps running
//! even though navigations are network-first.

use wasm_bindgen::{JsCast, JsValue};
use wasm_bindgen_futures::JsFuture;

/// Decide whether to reload: only when both versions are known, non-empty, and
/// differ. Never reload blind (unknown running build) — that would risk loops.
fn needs_reload(running: Option<&str>, deployed: Option<&str>) -> bool {
    matches!(
        (running, deployed),
        (Some(r), Some(d)) if !r.is_empty() && !d.is_empty() && r != d
    )
}

/// Build id baked into the running app, or None if not stamped (dev/unbuilt).
fn running_version() -> Option<String> {
    js_sys::Reflect::get(&js_sys::global(), &JsValue::from_str("__APP_VERSION__"))
        .ok()
        .and_then(|v| v.as_string())
        .filter(|s| !s.is_empty())
}

/// If the deployed build differs from the running one, reload to pick it up.
/// No-op when the running version is unknown (never reload blind) or offline.
pub async fn check_and_reload() {
    let Some(running) = running_version() else { return };
    let Some(window) = web_sys::window() else { return };

    // Cache-bust so neither the browser HTTP cache nor a proxy serves a stale id.
    let url = format!("/version.json?t={}", js_sys::Date::now() as u64);
    let Ok(resp_val) = JsFuture::from(window.fetch_with_str(&url)).await else {
        return; // offline / transient — retry on the next resume
    };
    let Ok(resp) = resp_val.dyn_into::<web_sys::Response>() else { return };
    if !resp.ok() {
        return;
    }
    let Ok(text_promise) = resp.text() else { return };
    let Ok(text_val) = JsFuture::from(text_promise).await else { return };
    let Some(text) = text_val.as_string() else { return };

    let deployed = serde_json::from_str::<serde_json::Value>(&text)
        .ok()
        .and_then(|j| j.get("v").and_then(|v| v.as_str()).map(str::to_string));
    let Some(deployed) = deployed else {
        leptos::logging::warn!("update: /version.json missing 'v': {text}");
        return;
    };

    if needs_reload(Some(&running), Some(&deployed)) {
        leptos::logging::log!("update: new build {deployed} (running {running}) — reloading");
        if let Some(loc) = web_sys::window().map(|w| w.location()) {
            let _ = loc.reload();
        }
    }
}

/// Fire-and-forget version check (used at launch and on resume).
pub fn check_background() {
    leptos::spawn_local(check_and_reload());
}

#[cfg(test)]
mod tests {
    use super::needs_reload;

    #[test]
    fn reloads_only_on_a_known_difference() {
        assert!(needs_reload(Some("a"), Some("b")));        // differ -> reload
        assert!(!needs_reload(Some("a"), Some("a")));       // same -> no
        assert!(!needs_reload(None, Some("b")));            // unknown running -> no
        assert!(!needs_reload(Some("a"), None));            // unknown deployed -> no
        assert!(!needs_reload(Some(""), Some("b")));        // empty running -> no
        assert!(!needs_reload(Some("a"), Some("")));        // empty deployed -> no
    }
}

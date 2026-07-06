//! In-app update detection (manual update, not auto-reload).
//!
//! `init.js` stamps the build id into `globalThis.__APP_VERSION__`, and the same
//! id is published at `/version.json`. We POLL that endpoint (cache-busted; the
//! service worker bypasses caching it) on launch + resume and, when it differs
//! from the running build, raise the reactive [`available`] flag. We do NOT
//! reload automatically — the user updates via Settings → «Версия» → Обновить
//! (which calls [`reload`]). The flag drives the red highlight on the Settings
//! nav icon and the Version row.

use std::cell::RefCell;

use leptos::*;
use wasm_bindgen::{JsCast, JsValue};
use wasm_bindgen_futures::JsFuture;

thread_local! {
    // Reactive "a newer build is deployed" flag, shared by the nav badge and the
    // Settings Version row. MUST be created at the ROOT (via init() in main, like
    // db::version) — never lazily inside a reactive closure, or it'd be owned by
    // that node and disposed on re-render, so set() from check() would hit a dead
    // handle and the nav wouldn't update.
    static UPDATE_AVAILABLE: RefCell<Option<RwSignal<bool>>> = const { RefCell::new(None) };
}

/// Create the shared flag in the root reactive scope. Call once from main()
/// before mounting, alongside db::init().
pub fn init() {
    UPDATE_AVAILABLE.with(|c| {
        if c.borrow().is_none() {
            *c.borrow_mut() = Some(create_rw_signal(false));
        }
    });
}

/// Reactive flag: true when a newer build than the running one is deployed.
pub fn available() -> RwSignal<bool> {
    UPDATE_AVAILABLE.with(|c| c.borrow().expect("update::init() must run before available()"))
}

/// True when both build ids are known, non-empty, and differ.
fn is_newer(running: Option<&str>, deployed: Option<&str>) -> bool {
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

/// The running build id for display ("—" when unknown).
pub fn current_version() -> String {
    running_version().unwrap_or_else(|| "—".to_string())
}

/// Poll `/version.json` and set the [`available`] flag accordingly. Does NOT
/// reload. No-op (leaves the flag untouched) when offline / the running version
/// is unknown, so a transient failure never clears a real "update available".
pub async fn check() {
    let Some(running) = running_version() else { return };
    let Some(window) = web_sys::window() else { return };

    // Cache-bust so neither the browser HTTP cache nor a proxy serves a stale id.
    let url = format!("/version.json?t={}", js_sys::Date::now() as u64);
    let Ok(resp_val) = JsFuture::from(window.fetch_with_str(&url)).await else {
        return; // offline / transient — retry on the next activation
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

    available().set(is_newer(Some(&running), Some(&deployed)));
}

/// Fire-and-forget version check (used at launch and on resume).
pub fn check_background() {
    leptos::spawn_local(check());
}

/// Reload to the deployed build — the manual "Обновить" action. Navigations are
/// network-first, so the reload pulls the new index.html/init.js/wasm.
pub fn reload() {
    if let Some(loc) = web_sys::window().map(|w| w.location()) {
        let _ = loc.reload();
    }
}

#[cfg(test)]
mod tests {
    use super::is_newer;

    #[test]
    fn flags_only_a_known_difference() {
        assert!(is_newer(Some("a"), Some("b"))); // differ -> available
        assert!(!is_newer(Some("a"), Some("a"))); // same -> no
        assert!(!is_newer(None, Some("b"))); // unknown running -> no
        assert!(!is_newer(Some("a"), None)); // unknown deployed -> no
        assert!(!is_newer(Some(""), Some("b"))); // empty running -> no
        assert!(!is_newer(Some("a"), Some(""))); // empty deployed -> no
    }
}

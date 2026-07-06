//! TEMP DIAGNOSTIC (remove once the iOS notification-receipt bug is fixed).
//!
//! Bridge into the page-side localStorage journal (`window.__pj`, defined in
//! index.html) so WASM code paths — the settings «Проверить уведомления» button,
//! the notification-receipt poll — leave breadcrumbs in the SAME journal the
//! Settings → «Разработка» panel displays. The journal lives in localStorage
//! only (no Cache Storage), so it keeps recording even if the page's Cache
//! access breaks — the suspected iOS failure mode.

use wasm_bindgen::JsCast;

pub fn note(msg: &str) {
    let Some(win) = web_sys::window() else { return };
    let Ok(f) = js_sys::Reflect::get(win.as_ref(), &"__pj".into()) else { return };
    let Ok(f) = f.dyn_into::<js_sys::Function>() else { return };
    let _ = f.call1(&wasm_bindgen::JsValue::NULL, &format!("WASM {msg}").into());
}

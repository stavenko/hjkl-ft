//! Bridge into the page-side localStorage journal (`window.__pj`, defined in
//! index.html) so WASM code paths — the settings «Проверить уведомления» button,
//! the notification-receipt poll — leave breadcrumbs in the SAME journal the
//! Settings → «Разработка» panel displays. The journal lives in localStorage
//! only (no Cache Storage): the page's Cache access detaches on iOS after a
//! push subscribe/receipt, a localStorage journal records through it
//! (docs/notification-receipt.md).

use wasm_bindgen::JsCast;

pub fn note(msg: &str) {
    let Some(win) = web_sys::window() else { return };
    let Ok(f) = js_sys::Reflect::get(win.as_ref(), &"__pj".into()) else { return };
    let Ok(f) = f.dyn_into::<js_sys::Function>() else { return };
    let _ = f.call1(&wasm_bindgen::JsValue::NULL, &format!("WASM {msg}").into());
}

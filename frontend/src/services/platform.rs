pub fn is_pwa() -> bool {
    let window = web_sys::window().expect("no window");

    let standalone = window
        .match_media("(display-mode: standalone)")
        .ok()
        .flatten()
        .map(|m| m.matches())
        .unwrap_or(false);

    let wco = window
        .match_media("(display-mode: window-controls-overlay)")
        .ok()
        .flatten()
        .map(|m| m.matches())
        .unwrap_or(false);

    let browser = window
        .match_media("(display-mode: browser)")
        .ok()
        .flatten()
        .map(|m| m.matches())
        .unwrap_or(true);

    let navigator_standalone = js_sys::Reflect::get(&window.navigator(), &"standalone".into())
        .ok()
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    leptos::logging::log!(
        "PWA detect: standalone={}, wco={}, browser={}, navigator.standalone={}",
        standalone, wco, browser, navigator_standalone
    );

    if standalone || wco || navigator_standalone {
        return true;
    }

    // If display-mode: browser is false, we're in some app mode
    !browser
}

pub fn pwa_dismissed() -> bool {
    let storage = web_sys::window()
        .expect("no window")
        .local_storage()
        .ok()
        .flatten()
        .expect("no localStorage");
    storage.get_item("pwa_dismissed").ok().flatten().is_some()
}

pub fn dismiss_pwa_prompt() {
    let storage = web_sys::window()
        .expect("no window")
        .local_storage()
        .ok()
        .flatten()
        .expect("no localStorage");
    storage.set_item("pwa_dismissed", "true").expect("localStorage write failed");
}

pub fn needs_pwa_prompt() -> bool {
    let pwa = is_pwa();
    let dismissed = pwa_dismissed();
    leptos::logging::log!("needs_pwa_prompt: is_pwa={}, dismissed={}", pwa, dismissed);
    !pwa && !dismissed
}

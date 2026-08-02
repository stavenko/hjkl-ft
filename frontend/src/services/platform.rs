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

/// Yandex Browser / the Yandex app's built-in browser (Android). The app cannot
/// work there at all, so several entry points route straight to the «open it in
/// Chrome» screen.
pub fn detect_platform_is_yandex() -> bool {
    crate::pages::pwa_prompt::detect_platform() == "android_yandex"
}

pub fn needs_pwa_prompt() -> bool {
    // Yandex Browser: ALWAYS. The app is unusable there (no passkey, no PWA), so
    // the «open it in Chrome» screen is shown on every launch and cannot be
    // dismissed — an earlier dismissal from the old screen must not hide it.
    if detect_platform_is_yandex() {
        return true;
    }
    let pwa = is_pwa();
    let dismissed = pwa_dismissed();
    leptos::logging::log!("needs_pwa_prompt: is_pwa={}, dismissed={}", pwa, dismissed);
    !pwa && !dismissed
}

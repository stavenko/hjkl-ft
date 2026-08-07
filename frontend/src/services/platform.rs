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

/// Приложение уже поставлено на этот рабочий стол.
///
/// Отметку ставит обработчик `appinstalled` в index.html — прямо перед тем, как
/// перезагрузить страницу. Перезагрузка стирает состояние, и без отметки
/// онбординг начинался бы заново.
pub fn pwa_installed() -> bool {
    web_sys::window()
        .and_then(|w| w.local_storage().ok().flatten())
        .and_then(|s| s.get_item("pwa_installed").ok().flatten())
        .is_some()
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

/// Mi Browser (Xiaomi) на Android. Отдельно от Яндекса и НЕ вместе с ним:
/// яндексовский путь выстрадан замерами (intent оттуда не работает, учим листу
/// «Поделиться»), а здесь intent как раз работает — проверено на устройстве.
/// Общая ветка склеила бы два разных лечения в одно и сломала бы рабочее.
pub fn detect_platform_is_mi() -> bool {
    crate::pages::pwa_prompt::detect_platform() == "android_mi"
}

pub fn needs_pwa_prompt() -> bool {
    // Yandex Browser: ALWAYS. The app is unusable there (no passkey, no PWA), so
    // the «open it in Chrome» screen is shown on every launch and cannot be
    // dismissed — an earlier dismissal from the old screen must not hide it.
    if detect_platform_is_yandex() {
        return true;
    }
    // Mi Browser — по той же причине и так же безусловно: ключ там не создать,
    // значит и пользоваться нечем, пока человек не перешёл в Chrome.
    if detect_platform_is_mi() {
        return true;
    }
    let pwa = is_pwa();
    let dismissed = pwa_dismissed();
    leptos::logging::log!("needs_pwa_prompt: is_pwa={}, dismissed={}", pwa, dismissed);
    !pwa && !dismissed
}

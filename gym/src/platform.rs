//! Что за окружение вокруг: установлено ли приложение, и не тот ли это браузер,
//! в котором работать нечем.
//!
//! Опознание браузера — из общего крейта `pwa-prompt`; здесь только вопросы,
//! которые задаёт приложение, и ответы в его словах.

/// Приложение запущено с иконки (standalone), а не во вкладке.
pub fn is_pwa() -> bool {
    let Some(win) = web_sys::window() else { return false };
    let mm = |q: &str| {
        win.match_media(q)
            .ok()
            .flatten()
            .map(|m| m.matches())
            .unwrap_or(false)
    };
    if mm("(display-mode: standalone)") || mm("(display-mode: window-controls-overlay)") {
        return true;
    }
    // iOS до сих пор отвечает только этим нестандартным полем.
    js_sys::Reflect::get(&win.navigator(), &wasm_bindgen::JsValue::from_str("standalone"))
        .ok()
        .and_then(|v| v.as_bool())
        .unwrap_or(false)
}

const KEY_INSTALLED: &str = "gym_pwa_installed";

/// Приложение уже поставлено на этот рабочий стол.
///
/// Отметку ставит обработчик `appinstalled` в index.html — прямо перед тем, как
/// перезагрузить страницу. Перезагрузка стирает состояние, и без отметки
/// инструкция по установке начиналась бы заново.
pub fn pwa_installed() -> bool {
    storage()
        .and_then(|s| s.get_item(KEY_INSTALLED).ok().flatten())
        .is_some()
}

/// Забыть, что приложение поставлено.
///
/// Нужно, когда человек говорит, что иконка так и не появилась: событие
/// `appinstalled` приходит РАНЬШЕ, чем Android доводит установку до конца, и если
/// та сорвалась, отметка врёт, а человек заперт на экране «всё готово».
pub fn clear_pwa_installed() {
    if let Some(s) = storage() {
        let _ = s.remove_item(KEY_INSTALLED);
    }
}

const KEY_DISMISSED: &str = "gym_pwa_dismissed";

/// Человек отказался ставить приложение (кнопка есть ТОЛЬКО на десктопе — см.
/// `install::InstallScreen`).
pub fn pwa_dismissed() -> bool {
    storage()
        .and_then(|s| s.get_item(KEY_DISMISSED).ok().flatten())
        .is_some()
}

pub fn dismiss_pwa_prompt() {
    if let Some(s) = storage() {
        let _ = s.set_item(KEY_DISMISSED, "true");
    }
}

// ── Аккаунт, который принесло с собой установленное приложение ───────────────

/// `?u=<user_id>` из адреса — несекретный идентификатор аккаунта, который
/// установленное приложение уносит в своём `start_url` (см. pwa-worker.js).
///
/// Он единственный способ узнать, ЧЕЙ это значок на домашнем экране, когда
/// сессии нет: на iOS у установленного приложения своё хранилище, и localStorage
/// вкладки ему недоступен.
pub fn param_user_id() -> Option<String> {
    let search = web_sys::window()?.location().search().ok()?;
    let params = web_sys::UrlSearchParams::new_with_str(&search).ok()?;
    params.get("u").filter(|s| !s.is_empty())
}

/// Перенацелить `<link rel="manifest">` на манифест ЭТОГО человека.
///
/// Зовётся сразу после входа — до того, как человеку покажут инструкцию по
/// установке. Браузер снимает манифест в момент «Добавить на экран Домой», и
/// снимок обязан быть уже персональным: иначе установленное приложение
/// запустится по `start_url = "/"`, без аккаунта, и на iOS ему неоткуда будет
/// узнать, кто им пользуется.
pub fn set_manifest_user(user_id: &str) {
    let Some(doc) = web_sys::window().and_then(|w| w.document()) else { return };
    if let Ok(Some(link)) = doc.query_selector("link[rel=manifest]") {
        let _ = link.set_attribute("href", &format!("/manifest.json?u={user_id}"));
    }
}

fn storage() -> Option<web_sys::Storage> {
    web_sys::window()?.local_storage().ok()?
}

/// Яндекс.Браузер (и встроенный браузер приложения Яндекса) на Android. Ключ там
/// не создать — `navigator.credentials` отсутствует, — и приложение не поставить.
pub fn is_yandex() -> bool {
    pwa_prompt::detect_platform() == "android_yandex"
}

/// Mi Browser (Xiaomi). Отдельно от Яндекса и НЕ вместе с ним: лечение разное.
/// Оттуда intent в Chrome срабатывает, поэтому человеку нужна одна кнопка, а не
/// инструкция.
pub fn is_mi() -> bool {
    pwa_prompt::detect_platform() == "android_mi"
}

/// Samsung Internet. Лечение то же, что у Mi: одна кнопка в Chrome.
pub fn is_samsung() -> bool {
    pwa_prompt::detect_platform() == "android_samsung"
}

/// Браузер не опознан ни по одному признаку. Инструкции по установке для него
/// нет и быть не может — мы не знаем ни его меню, ни его пунктов.
pub fn is_unknown() -> bool {
    matches!(pwa_prompt::detect_platform(), "unknown" | "ios_unknown")
}

/// Браузер, в котором приложению делать нечего: ключ там не завести, приложение
/// не поставить. Такой экран показывается ДО входа и не закрывается.
pub fn is_dead_end_browser() -> bool {
    is_yandex() || is_mi() || is_samsung() || is_unknown()
}

/// Нужно ли показывать экран установки.
///
/// Порядок проверок не случаен:
/// * тупиковый браузер — всегда (там своя ветка с уходом в Chrome);
/// * запущены С ИКОНКИ — установка позади, дальше приложение;
/// * отметка об установке стоит, а мы всё ещё во вкладке — экран НУЖЕН, и
///   именно он говорит «открывайте с иконки». Пустить такого человека внутрь
///   вкладки значит оставить его пользоваться браузером, ради ухода из которого
///   он приложение и ставил;
/// * иначе — инструкция, пока человек от неё не отказался (кнопка отказа есть
///   только на десктопе).
pub fn needs_install_screen() -> bool {
    if is_dead_end_browser() {
        return true;
    }
    if is_pwa() {
        return false;
    }
    if pwa_installed() {
        return true;
    }
    !pwa_dismissed()
}

//! Обновление приложения — вручную, а не самоперезагрузкой.
//!
//! Перенесено из приложения худеющего (frontend/src/services/update.rs).
//!
//! Как устроено: `scripts/build-shell.sh` штампует идентификатор сборки в
//! `globalThis.__APP_VERSION__` и публикует тот же идентификатор в
//! `/version.json`. Приложение опрашивает этот адрес (сервис-воркер его никогда
//! не кэширует) при запуске и при возвращении на передний план; если выложенный
//! идентификатор отличается от запущенного — поднимается реактивный флаг
//! [`available`].
//!
//! Само НЕ перезагружается: человек обновляется кнопкой в Настройках, а флаг
//! только красит точку на иконке меню и строку «Версия». Молча обновляемся
//! ровно в одном месте — на экранах ДО приложения (см. [`apply_before_app`]),
//! потому что оттуда в Настройки не попасть.

use std::cell::RefCell;

use leptos::*;
use wasm_bindgen::{JsCast, JsValue};
use wasm_bindgen_futures::JsFuture;

thread_local! {
    // Реактивный флаг «выложена сборка новее запущенной». Заводится в КОРНЕВОЙ
    // области видимости (через init() из main) — никогда лениво внутри
    // реактивного замыкания: там сигналом владел бы этот узел, и на его
    // перерисовке сигнал бы уничтожился, а set() из check() бил бы по мёртвой
    // ручке — меню бы не обновилось.
    static UPDATE_AVAILABLE: RefCell<Option<RwSignal<bool>>> = const { RefCell::new(None) };
}

/// Завести общий флаг в корневой области. Один раз из main() до монтирования.
pub fn init() {
    UPDATE_AVAILABLE.with(|c| {
        if c.borrow().is_none() {
            *c.borrow_mut() = Some(create_rw_signal(false));
        }
    });
}

/// Реактивный флаг: выложена сборка новее запущенной.
pub fn available() -> RwSignal<bool> {
    UPDATE_AVAILABLE.with(|c| c.borrow().expect("update::init() должен пройти до available()"))
}

/// Истина, только когда ОБА идентификатора известны, непусты и различаются.
fn is_newer(running: Option<&str>, deployed: Option<&str>) -> bool {
    matches!(
        (running, deployed),
        (Some(r), Some(d)) if !r.is_empty() && !d.is_empty() && r != d
    )
}

/// Идентификатор запущенной сборки, или None, если штампа нет (dev/несобранное).
fn running_version() -> Option<String> {
    js_sys::Reflect::get(&js_sys::global(), &JsValue::from_str("__APP_VERSION__"))
        .ok()
        .and_then(|v| v.as_string())
        .filter(|s| !s.is_empty())
}

/// Идентификатор запущенной сборки для показа («—», если неизвестен).
pub fn current_version() -> String {
    running_version().unwrap_or_else(|| "—".to_string())
}

/// Опросить `/version.json` и выставить флаг [`available`]. НЕ перезагружает.
/// Офлайн или без штампа версии — ничего не делает и флаг НЕ трогает: разовый
/// сбой сети не должен погасить настоящее «есть обновление».
pub async fn check() {
    let Some(running) = running_version() else { return };
    let Some(window) = web_sys::window() else { return };

    // Ломаем кэш, чтобы ни браузер, ни промежуточный прокси не отдали старый id.
    let url = format!("/version.json?t={}", js_sys::Date::now() as u64);
    let Ok(resp_val) = JsFuture::from(window.fetch_with_str(&url)).await else {
        return; // офлайн или разовый сбой — повторим при следующем поводе
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
        leptos::logging::warn!("update: в /version.json нет поля 'v': {text}");
        return;
    };

    available().set(is_newer(Some(&running), Some(&deployed)));
}

/// Проверка без ожидания результата (запуск и возвращение на передний план).
pub fn check_background() {
    leptos::spawn_local(check());
}

/// Перезагрузиться на выложенную сборку — то самое «Обновить». Навигации идут
/// сеть-вперёд, поэтому перезагрузка притянет новые index.html/init.js/wasm.
pub fn reload() {
    // Взводим сторож ДО перезагрузки: `location.reload()` ходит в сеть и может
    // застрять — и до навигации, и уже на экране загрузки новой страницы. Сторож
    // живёт в index.html обычным JS, не в WASM, потому что во втором случае
    // никакого Rust ещё нет; отметка о начале лежит в sessionStorage и переживает
    // переход. Через 15 секунд он предлагает выход.
    call_js("__rnUpdateArm");
    if let Some(loc) = web_sys::window().map(|w| w.location()) {
        let _ = loc.reload();
    }
}

/// Приложение поднялось — обновление состоялось, сторожить нечего.
pub fn note_app_started() {
    call_js("__rnUpdateDone");
}

/// Позвать функцию, объявленную в index.html. Её отсутствие не ошибка: страницу
/// могли открыть из старого кэша, где этого сторожа ещё нет.
fn call_js(name: &str) {
    let Some(win) = web_sys::window() else { return };
    let Ok(f) = js_sys::Reflect::get(&win, &JsValue::from_str(name)) else { return };
    if let Ok(f) = f.dyn_into::<js_sys::Function>() {
        let _ = f.call0(&JsValue::NULL);
    }
}

/// Ключ вкладки: обновление уже применялось само.
const AUTO_APPLIED_KEY: &str = "gym_update_auto_applied";

/// Применить обновление молча — для экранов ДО приложения (вход, установка).
///
/// Обновиться там иначе нельзя: единственная кнопка «Обновить» живёт в
/// Настройках, а из инструкции по установке в Настройки не попасть. Человек со
/// старой сборкой в кэше застревает на ней навсегда — ровно в том месте, где
/// свежая нужнее всего. Терять на этих экранах нечего: ни введённого текста, ни
/// несохранённого.
///
/// Ровно один раз за вкладку. Если новая сборка почему-то не приезжает, второй
/// заход не должен превратиться в бесконечную перезагрузку.
pub fn apply_before_app() {
    let Some(window) = web_sys::window() else { return };
    let Ok(Some(storage)) = window.session_storage() else { return };
    if storage.get_item(AUTO_APPLIED_KEY).ok().flatten().is_some() {
        return;
    }
    let _ = storage.set_item(AUTO_APPLIED_KEY, "1");
    leptos::logging::log!("update: применяем новую сборку до входа в приложение");
    reload();
}

#[cfg(test)]
mod tests {
    use super::is_newer;

    #[test]
    fn flags_only_a_known_difference() {
        assert!(is_newer(Some("a"), Some("b"))); // различаются → есть обновление
        assert!(!is_newer(Some("a"), Some("a"))); // одинаковы → нет
        assert!(!is_newer(None, Some("b"))); // запущенная неизвестна → нет
        assert!(!is_newer(Some("a"), None)); // выложенная неизвестна → нет
        assert!(!is_newer(Some(""), Some("b"))); // пустая запущенная → нет
        assert!(!is_newer(Some("a"), Some(""))); // пустая выложенная → нет
    }
}

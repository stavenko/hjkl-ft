//! Удалить с устройства базу `hjkl-ft`.
//!
//! Это была device-global «бутстрап»-база: приложение открывало её на старте, пока
//! не знало пользователя, и до входа она была активной. Хранить в ней было нечего —
//! устройство живёт в localStorage, персону спрашивают виджеты уже после входа, —
//! зато туда оседали служебные записи: версия миграций, отметки о выполненных
//! backfill'ах, штампы `updated_at`. Всё это принадлежит базе пользователя.
//!
//! Теперь база открывается только для известного пользователя, а `hjkl-ft` остаётся
//! на устройстве мёртвым грузом. Удаляем.
//!
//! Идемпотентна: отсутствующую базу `deleteDatabase` удаляет молча и успешно.

use wasm_bindgen::JsCast;

pub const VERSION: u32 = 2;
pub const DESCRIPTION: &str = "удалить device-global базу hjkl-ft";

const LEGACY_DB: &str = "hjkl-ft";

pub async fn script() -> Result<(), String> {
    let factory = web_sys::window()
        .ok_or("нет window")?
        .indexed_db()
        .map_err(|e| format!("indexedDB недоступна: {e:?}"))?
        .ok_or("indexedDB недоступна")?;
    let req = factory
        .delete_database(LEGACY_DB)
        .map_err(|e| format!("deleteDatabase({LEGACY_DB}): {e:?}"))?;

    // `onblocked` НЕ ошибка: он означает, что базу держит открытой другая вкладка.
    // Удаление состоится, когда та закроется, а нам достаточно того, что запрос
    // принят — иначе миграция вставала бы намертво у людей с двумя вкладками.
    let done = wasm_bindgen_futures::JsFuture::from(js_sys::Promise::new(&mut |resolve, reject| {
        let ok = resolve.clone();
        let on_ok = wasm_bindgen::closure::Closure::once_into_js(move || {
            let _ = ok.call0(&wasm_bindgen::JsValue::NULL);
        });
        let on_blocked = resolve.clone();
        let on_block = wasm_bindgen::closure::Closure::once_into_js(move || {
            leptos::logging::warn!(
                "удаление {LEGACY_DB} отложено: база открыта в другой вкладке"
            );
            let _ = on_blocked.call0(&wasm_bindgen::JsValue::NULL);
        });
        let on_err = wasm_bindgen::closure::Closure::once_into_js(move || {
            let _ = reject.call1(
                &wasm_bindgen::JsValue::NULL,
                &wasm_bindgen::JsValue::from_str("deleteDatabase failed"),
            );
        });
        req.set_onsuccess(Some(on_ok.unchecked_ref()));
        req.set_onblocked(Some(on_block.unchecked_ref()));
        req.set_onerror(Some(on_err.unchecked_ref()));
    }))
    .await;

    match done {
        Ok(_) => {
            leptos::logging::log!("миграция 2: база {LEGACY_DB} удалена");
            Ok(())
        }
        Err(e) => Err(format!("не удалось удалить {LEGACY_DB}: {e:?}")),
    }
}

//! Засев базы для испытаний — ТОЛЬКО на тестовом домене.
//!
//! Испытания интерфейса упираются в одно и то же: данные, положенные в базу до
//! запуска приложения, сметаются миграциями. Миграция открывает базу своей версией,
//! перебирает хранилища и приводит строки к нынешней схеме — и то, что тест успел
//! записать, до этого не доживает. Обходить это подгонкой версии в тесте не выходит:
//! версия меняется в приложении, а тест про это узнаёт падением.
//!
//! Поэтому засев переносится ВНУТРЬ приложения и происходит ПОСЛЕ миграций. Тест
//! кладёт данные в localStorage и перезагружает страницу; приложение, увидев их,
//! раскладывает по хранилищам своей рукой — теми же путями, что и обычную запись.
//!
//! Три ограничения, и каждое здесь не для порядка:
//!
//! 1. Только на тестовом домене. На боевом этот путь не существует: даже если ключ
//!    в localStorage появится, он будет проигнорирован.
//! 2. Только известные хранилища. Опечатка в названии — громкая ошибка в консоли, а
//!    не тихое ничего: тест, засевающий не туда, должен падать, а не проходить.
//! 3. Ключ СЪЕДАЕТСЯ. Иначе тест, удаливший запись и перезагрузивший страницу,
//!    получил бы её обратно и не понял бы, почему удаление «не работает».
//!
//! Формат значения — то, что человек прочтёт с одного взгляда: имя хранилища и
//! строки, которые в него положить.
//!
//! ```json
//! {
//!   "foods":     [{ "id": "f1", "name": "Творог", "kcal": 96, ... }],
//!   "diary":     [{ "id": "d1", "food_id": "f1", "date": "2026-09-03", ... }],
//!   "app_flags": [{ "key": "feature.lazy_food", "value": "true" }]
//! }
//! ```

use crate::services::db;

/// Домен, на котором засев разрешён. Захардкожен намеренно: список из настроек
/// можно подменить, а строку в коде — только новой сборкой.
const TEST_HOST: &str = "renorma-fit-dev.pages.dev";

/// Ключ в localStorage, куда испытание кладёт данные.
pub const SEED_KEY: &str = "ft_test_seed";

/// Хранилища, в которые засев вправе писать. Список нарочно СВОЙ, а не взятый из
/// `db::ALL_STORES`: служебные хранилища синхронизации засевать нечего, и лучше
/// дописать сюда строку осознанно, чем однажды затереть журнал мутаций.
const SEEDABLE: &[&str] = &[
    "foods", "diary", "recipes", "recipe_ingredients", "goals", "food_drafts",
    "weight_entries", "step_entries", "summaries", "profile", "app_flags",
    "planka_history", "images",
];

/// Тот ли это домен, где засев разрешён.
fn on_test_host() -> bool {
    web_sys::window()
        .and_then(|w| w.location().hostname().ok())
        .is_some_and(|h| h == TEST_HOST)
}

fn storage() -> Option<web_sys::Storage> {
    web_sys::window().and_then(|w| w.local_storage().ok().flatten())
}

/// Разложить засев по хранилищам. Возвращает число положенных строк.
///
/// Зовётся из `app.rs` СРАЗУ ПОСЛЕ миграций — в этом весь смысл: до миграций
/// положенное не выживет.
pub async fn apply() -> usize {
    if !on_test_host() {
        return 0;
    }
    let Some(store) = storage() else { return 0 };
    let Ok(Some(raw)) = store.get_item(SEED_KEY) else { return 0 };
    // Ключ съедается ДО раскладки: если раскладка упадёт на середине, повторный
    // запуск не должен снова наткнуться на тот же битый засев.
    let _ = store.remove_item(SEED_KEY);

    let parsed: serde_json::Value = match serde_json::from_str(&raw) {
        Ok(v) => v,
        Err(e) => {
            leptos::logging::error!("засев: не разбирается JSON: {e}");
            return 0;
        }
    };
    let Some(map) = parsed.as_object() else {
        leptos::logging::error!("засев: ожидался объект «хранилище → строки»");
        return 0;
    };

    let mut written = 0usize;
    let mut touched_app_flags = false;
    for (store_name, rows) in map {
        if !SEEDABLE.contains(&store_name.as_str()) {
            // Громко: тест, засевающий не туда, должен падать, а не проходить.
            leptos::logging::error!("засев: хранилище «{store_name}» засевать нельзя или его нет");
            continue;
        }
        let Some(rows) = rows.as_array() else {
            leptos::logging::error!("засев: «{store_name}» — ожидался массив строк");
            continue;
        };
        for row in rows {
            // `put_untracked`, а не `put`: засев это оснастка испытания, а не
            // действие человека, и в журнал синхронизации ему попадать незачем —
            // иначе выдуманные строки уехали бы на сервер.
            db::put_untracked(store_name, row).await;
            written += 1;
        }
        touched_app_flags |= store_name == "app_flags";
    }

    // Флаги читаются из памяти, а не из базы (см. `app_flags`), поэтому засеянный
    // флаг без перечитывания не подействовал бы до следующего запуска — а тест
    // именно им и включает новую возможность.
    if touched_app_flags {
        crate::services::app_flags::reload().await;
    }
    leptos::logging::log!("засев: положено строк {written}");
    written
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sluzhebnye_hranilishcha_zasevat_nelzya() {
        // Журнал мутаций и метаданные синхронизации — не оснастка испытания, и
        // затирать их засевом нельзя.
        assert!(!SEEDABLE.contains(&"_sync_meta"));
        assert!(!SEEDABLE.contains(&"deletions"));
        assert!(!SEEDABLE.contains(&"support_outbox"));
    }

    #[test]
    fn to_chto_nuzhno_ispytaniyam_zasevat_mozhno() {
        for name in ["foods", "diary", "app_flags", "images"] {
            assert!(SEEDABLE.contains(&name), "{name} должно быть засеваемым");
        }
    }

    #[test]
    fn test_host_zahardkozhen_i_ne_boevoj() {
        assert_eq!(TEST_HOST, "renorma-fit-dev.pages.dev");
        assert!(!TEST_HOST.contains("renorma.app"), "боевой домен здесь стоять не должен");
    }
}

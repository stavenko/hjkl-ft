//! Вычистить из базы поля снятых классификаторов.
//!
//! «Красное мясо», «Яйца», «Низкокалорийный перекус» и «Жидкие калории» сняты:
//! модель их больше не спрашивают, ничего от них не считается, а в форме правки
//! продукта они висели и ВРАЛИ — у кижуча стояло «Красное мясо: да» от прежнего
//! прохода. Поля убраны из `api_types::Food`, но старые значения остались лежать
//! в IndexedDB: serde просто игнорирует незнакомые ключи при чтении.
//!
//! Здесь мы их физически стираем. Разбор в `Food` отбрасывает лишние ключи, а
//! обратная запись сохраняет уже очищенную строку.
//!
//! Пишем ТОЛЬКО те строки, где мусор действительно был: холостая перезапись всей
//! таблицы ушла бы в журнал синхронизации и превратила бы ближайший синк в
//! пересылку всей базы.

use api_types::Food;

use crate::services::db;

pub const VERSION: u32 = 10;
pub const DESCRIPTION: &str = "стереть поля снятых классификаторов";

const DEAD: [&str; 4] = ["is_snack", "is_liquid_cal", "is_egg", "is_red_meat"];

pub async fn script() -> Result<(), String> {
    // Читаем СЫРЫЕ значения: только так видно ключи, которых в типе уже нет.
    let raw: Vec<serde_json::Value> = db::list_all("foods").await;
    let mut cleaned = 0_usize;
    for value in raw {
        if !DEAD.iter().any(|k| value.get(k).is_some_and(|v| !v.is_null())) {
            continue;
        }
        let food: Food = match serde_json::from_value(value.clone()) {
            Ok(f) => f,
            // Строка, которую не разобрать, — не наше дело чинить молча: пропускаем
            // её и говорим об этом вслух, но проход не обрываем.
            Err(e) => {
                leptos::logging::error!("миграция 10: продукт не разобрался: {e}");
                continue;
            }
        };
        db::put("foods", &food).await;
        cleaned += 1;
    }
    leptos::logging::log!("миграция 10: очищено продуктов: {cleaned}");
    if cleaned > 0 {
        crate::services::sync::push_background();
    }
    Ok(())
}

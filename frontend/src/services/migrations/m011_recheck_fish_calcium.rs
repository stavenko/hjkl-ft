//! Перезапросить кальций там, где его назначила сломанная рыбная строка.
//!
//! В таблице категорий была единственная рыбная строка — `fish_with_bones`
//! (120–450 мг), и модель складывала в неё ЛЮБУЮ рыбу: кижуч и треска получали
//! 285 мг — ровно середину диапазона — при настоящих 36 и 16. Замер живого пути:
//! `scripts/measure-calcium-rows.mjs`, 10/12 до правки и 12/12 после.
//!
//! Строку разделили на две, но у людей в базе остались прежние числа, а фоновый
//! проход их не трогает: ключ «Кальций» есть, значит выяснено.
//!
//! Стираем кальций РОВНО у тех продуктов, где стоит 285 — отпечаток той самой
//! середины. Сардинам это ничем не грозит: их перезапросят и они получат свои 285
//! обратно, уже законно. Сплошной сброс кальция был бы честнее по форме, но
//! отправил бы к модели весь список продуктов у каждого человека и заодно стёр бы
//! значения, вписанные руками.

use api_types::Food;

use crate::services::{db, indicators::N_CALCIUM};

pub const VERSION: u32 = 11;
pub const DESCRIPTION: &str = "перезапросить кальций у рыбы";

/// Середина сломанной строки — единственное число, которое она порождала.
const BROKEN_VALUE: f64 = 285.0;

pub async fn script() -> Result<(), String> {
    let foods: Vec<Food> = db::list_all("foods").await;
    let mut cleared = 0_usize;
    for mut food in foods {
        let hit = food
            .nutrients
            .get(N_CALCIUM)
            .is_some_and(|v| (*v - BROKEN_VALUE).abs() < 1e-6);
        if !hit {
            continue;
        }
        food.nutrients.remove(N_CALCIUM);
        food.updated_at = crate::services::local::now();
        db::put("foods", &food).await;
        cleared += 1;
    }
    leptos::logging::log!("миграция 11: кальций сброшен у продуктов: {cleared}");
    if cleared > 0 {
        // Пересчитать блюда, куда эти продукты входят, и разослать изменения.
        crate::services::sync::push_background();
    }
    Ok(())
}

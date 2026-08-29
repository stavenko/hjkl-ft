//! Спросить овощ/фрукт заново — третий раз, и по причине, которой не было в
//! [`super::m017_reask_veg_fruit`] и [`super::m021_reask_veg_fruit_again`].
//!
//! Тогда чинился промпт, потом опознание перед ним. Теперь сменилось САМО ПРАВИЛО:
//! до 21 августа «выросло под землёй» выбрасывало из планки все корнеплоды, и
//! морковь, свёкла, редиска и картофель получали «нет». Правило отменено —
//! корнеплоды считаются, признак корня остался только в разборе продукта, — но
//! ответы, записанные до правки, так и лежат.
//!
//! Сами по себе они не исправятся никогда: [`crate::services::classify`] отбирает
//! продукты условием `is_veg_fruit.is_none()`, то есть спрашивает только тех, у
//! кого признака нет вовсе. Записанное «нет» переспрашивать некому — 300 г моркови
//! и 300 г картофеля в дневнике так и остаются нулём овощей.
//!
//! Стираем у ВСЕХ продуктов, как и в прошлые два раза: снаружи не видно, какой
//! ответ дан старым правилом, а какой новым. Фоновый проход переспросит по одному
//! запросу на продукт, разово.
//!
//! Блюда не трогаются: у них признака нет вовсе, овощи и фрукты блюда считаются из
//! состава.
//!
//! Дни пересчитываются: признак входит в дневные индикаторы, и без сброса кэша
//! экран остался бы с прежними цветами.

use api_types::Food;

use crate::services::db;

pub const VERSION: u32 = 24;
pub const DESCRIPTION: &str = "переспросить овощ/фрукт: корнеплоды теперь в планке";

pub async fn script() -> Result<(), String> {
    let foods: Vec<Food> = db::list_all("foods").await;
    let mut cleared = 0_usize;
    let mut touched: Vec<String> = Vec::new();
    for mut food in foods {
        if food.is_recipe || food.is_veg_fruit.is_none() {
            continue;
        }
        food.is_veg_fruit = None;
        food.updated_at = crate::services::local::now();
        db::put("foods", &food).await;
        touched.push(food.id.clone());
        cleared += 1;
    }
    leptos::logging::log!("миграция 24: овощ/фрукт сброшен у продуктов: {cleared}");
    if cleared > 0 {
        for id in touched {
            crate::services::indicators::invalidate_food(&id).await;
        }
        crate::services::sync::push_background();
    }
    Ok(())
}

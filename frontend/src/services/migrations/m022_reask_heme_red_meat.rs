//! Спросить заново ГЕМ и КРАСНОЕ МЯСО.
//!
//! Оба признака ставились на опознании, которому мы больше не верим: гейт рубил
//! живые имена, а там, где всё же пропускал, версия продукта нередко строилась на
//! догадке. Признаки эти дорогие — на них держатся недельная планка мяса, её
//! шкала и два индикатора, — и ошибка в них видна человеку прямо на экране.
//!
//! Стираем У ВСЕХ продуктов: какой именно ответ испорчен, снаружи не видно, а
//! половинчатый сброс оставил бы половину вранья. Фоновый проход переспросит по
//! одному запросу на продукт, разово.
//!
//! Блюда не трогаются: у них признаков нет вовсе — мясо и гем блюда считаются из
//! состава.
//!
//! Дни пересчитываются: оба признака входят в недельные величины, и без сброса
//! кэша шкала осталась бы со старыми числами.

use api_types::Food;

use crate::services::db;

pub const VERSION: u32 = 22;
pub const DESCRIPTION: &str = "переспросить гем и красное мясо после починки опознания";

pub async fn script() -> Result<(), String> {
    let foods: Vec<Food> = db::list_all("foods").await;
    let mut cleared = 0_usize;
    let mut touched: Vec<String> = Vec::new();
    for mut food in foods {
        if food.is_recipe {
            continue;
        }
        let had = food.is_heme.is_some() || food.is_red_meat.is_some();
        if !had {
            continue;
        }
        food.is_heme = None;
        food.is_red_meat = None;
        food.updated_at = crate::services::local::now();
        db::put("foods", &food).await;
        touched.push(food.id.clone());
        cleared += 1;
    }
    leptos::logging::log!("миграция 22: гем и красное мясо сброшены у продуктов: {cleared}");
    if cleared > 0 {
        for id in touched {
            crate::services::indicators::invalidate_food(&id).await;
        }
        crate::services::sync::push_background();
    }
    Ok(())
}

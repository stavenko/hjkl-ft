//! Перенести калорийную планку из `goals` в историю — последний раз.
//!
//! Планка живёт в истории планок, и запись в `goals` была её зеркалом: там она
//! оказывалась потому, что когда-то планкой и была. Приложение перестало эту
//! запись читать, а значит человек, у которого история пуста, а цель есть,
//! остался бы без планки вовсе — и приложение решило бы, что её пора поставить
//! ВПЕРВЫЕ, заменив его число свежерассчитанным.
//!
//! [`super::m003_seed_planka_history`] делал то же самое, но давно и один раз: у
//! того, кто с тех пор менял устройство или чинил базу, история могла опустеть
//! снова. Поэтому здесь не «завести историю», а «проверить и дописать»: есть
//! запись — не трогаем, нет — берём из цели, датируя днём её создания.
//!
//! После этого `goals` не читает никто.
//!
//! TODO: УДАЛИТЬ эту миграцию вместе с объявлением store в `db.rs` — она нужна
//! ровно до тех пор, пока у кого-то может остаться планка в старых целях.

use crate::services::local;

pub const VERSION: u32 = 25;
pub const DESCRIPTION: &str = "калорийная планка переезжает из целей в историю";

pub async fn script() -> Result<(), String> {
    if local::planka_history(local::PLANKA_CALORIES).await.iter().any(|e| e.amount > 0.0) {
        return Ok(());
    }
    let Some(goal) = local::legacy_calorie_goal_row().await else {
        return Ok(());
    };
    let today = local::today_date().format("%Y-%m-%d").to_string();
    let from = goal.created_at.get(0..10).unwrap_or(&today).to_string();
    local::seed_planka_at(local::PLANKA_CALORIES, &from, goal.amount).await;
    Ok(())
}

//! Завести историю планок из того, что известно сейчас.
//!
//! История планок появилась позже самих планок, поэтому у всех, кто уже пользуется
//! приложением, она пуста: в профиле лежит текущая шаговая планка, в `goals` —
//! текущая калорийная, а с какого дня они действуют, нигде не записано.
//!
//! Ставим по одной записи на вид, датируя её тем днём, с которого планка МОГЛА
//! действовать: калорийную — днём создания цели, шаговую — днём открытия недели
//! активности (тогда её и назначают). Это не восстанавливает промежуточные
//! изменения — их взять неоткуда, — но дальше история пишется на каждой установке, и
//! дни судятся по своей планке, а не по сегодняшней.

use crate::services::{app_flags, local, profile};

pub const VERSION: u32 = 3;
pub const DESCRIPTION: &str = "завести историю планок из текущих значений";

pub async fn script() -> Result<(), String> {
    let today = local::today_date().format("%Y-%m-%d").to_string();

    // ── Калории: дата создания цели ──
    if let Some(goal) = local::legacy_goals()
        .await
        .into_iter()
        .find(|g| g.nutrient == "Calories" && g.amount > 0.0)
    {
        let from = goal.created_at.get(0..10).unwrap_or(&today).to_string();
        local::seed_planka_at(local::PLANKA_CALORIES, &from, goal.amount).await;
    }

    // ── Шаги: день открытия недели активности, иначе первый день с шагами ──
    if let Some(planka) = profile::get_steps_planka() {
        let from = match app_flags::get(crate::services::indicators::STEPS_GATE_OPEN_KEY) {
            Some(d) if d.len() >= 10 => d,
            _ => local::list_step_entries()
                .await
                .into_iter()
                .map(|e| e.date)
                .min()
                .unwrap_or_else(|| today.clone()),
        };
        local::seed_planka_at(local::PLANKA_STEPS, &from, planka).await;
    }

    // ── Белок: норма от ПЕРВОГО веса, с даты той записи ──
    // Норма считается от веса, а он меняется; исторические значения восстановить
    // нельзя, но отправная точка — первое взвешивание — известна.
    if let Some(first) = local::list_weight_entries().await.into_iter().next() {
        let target = profile::protein_target_from_profile(first.weight_kg).await;
        if target > 0 {
            local::seed_planka_at(local::PLANKA_PROTEIN, &first.date, target as f64).await;
        }
    }

    Ok(())
}

//! То же, что миграция 13, но для ШАГОВ и БЕЛКА.
//!
//! Правило «день до первой планки судить нечем» сперва завели только для калорий —
//! с них начался разбор. Планки шагов и белка точно так же движутся и точно так же
//! имеют историю, а значит и та же беда: до первой установки день сравнивался с
//! ТЕКУЩЕЙ нормой и мог оказаться недобором задним числом.
//!
//! Отдельная миграция, а не правка тринадцатой: та у части людей уже прошла, и
//! дополненный скрипт к ним бы не вернулся.
//!
//! Нормы, не зависящие от веса и его динамики (кальций, овощи, жиры), здесь ни при
//! чём: они едины во времени, их дни судятся текущей нормой законно.

use crate::services::{db, local};

pub const VERSION: u32 = 14;
pub const DESCRIPTION: &str = "снять вердикты по шагам и белку за дни без планки";

#[derive(serde::Deserialize)]
struct Day {
    date: String,
}

/// Вид планки и кэш его вердиктов.
const PAIRS: &[(&str, &str)] = &[
    (local::PLANKA_STEPS, "ind_steps"),
    (local::PLANKA_PROTEIN, "ind_protein"),
];

pub async fn script() -> Result<(), String> {
    for (kind, store) in PAIRS {
        // Первая установка. Её нет вовсе — значит планки не было никогда, и ни один
        // день осуждать было не по чему.
        let first = local::planka_history(kind)
            .await
            .into_iter()
            .next()
            .map(|e| e.date);

        let days: Vec<Day> = db::list_all(store).await;
        let mut cleared = 0_usize;
        for day in days {
            let before_planka = match &first {
                Some(from) => day.date.as_str() < from.as_str(),
                None => true,
            };
            if before_planka {
                db::delete(store, &day.date).await;
                cleared += 1;
            }
        }
        leptos::logging::log!(
            "миграция 14: {kind} — снято вердиктов за дни без планки: {cleared} (планка с {first:?})"
        );
    }
    Ok(())
}

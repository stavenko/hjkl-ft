//! Яйца — недельная планка ШТУК, которую надо НАБРАТЬ.
//!
//! Обратная по смыслу соседке: у красного мяса недельная величина — потолок, здесь
//! это минимум. Семь яиц в неделю надёжно закрывают холин и селен, добавляют
//! качественного белка и занимают всего восемь процентов недельного лимита
//! насыщенных жиров. Одно яйцо в день — привычный ориентир, а не предел.
//!
//! **Считается через БЕЛОК, а не через штуки в дневнике.** Человек записывает
//! «Яичница глазунья, 120 г» или «Омлет, 200 г» — сколько там яиц, из записи не
//! видно, а вес блюда включает масло, молоко и всё остальное. Белок же при готовке
//! никуда не девается: в одном яйце его 6.3 г, значит по белку яичных продуктов
//! число яиц восстанавливается однозначно. Тот же приём, что у красного мяса с его
//! сырым эквивалентом, и по той же причине.
//!
//! Заодно это решает вопрос с блюдами: в запеканке засчитываются яйца, а не всё
//! блюдо, — блюда раскрываются по составу общим механизмом [`local::tag_protein_g_on`].
//!
//! **Что считается яйцом.** Только продукты, помеченные признаком `is_egg`: яйца в
//! любом виде — варёные, жареные, копчёные. Майонез и салаты с яйцом сюда не идут:
//! доля яйца в них не восстанавливается ниоткуда, и признак у них не ставится.

use chrono::{Duration, NaiveDate};

use super::{app_flags, local};
use api_types::Food;

/// App-flag: неделя яиц открыта (шкала и индикатор видны).
pub const EGG_UNLOCKED_KEY: &str = "egg_week_unlocked";

/// App-flag: день, от которого катится сетка недель яиц.
pub const EGG_WEEK_OPEN_KEY: &str = "egg_week_opened_at";

/// Недельная планка в ЯЙЦАХ. Минимум, а не потолок.
pub const WEEKLY_MIN_EGGS: f64 = 7.0;

/// Белок одного яйца, граммы. Столовое яйцо первой категории даёт 6.3 г.
pub const PROTEIN_PER_EGG_G: f64 = 6.3;

/// Недельная планка в граммах БЕЛКА — то, в чём на самом деле идёт счёт.
pub fn weekly_min_protein_g() -> f64 {
    WEEKLY_MIN_EGGS * PROTEIN_PER_EGG_G
}

/// Сколько яиц стоит за таким количеством яичного белка.
pub fn eggs_from_protein(protein_g: f64) -> f64 {
    protein_g / PROTEIN_PER_EGG_G
}

/// Идёт ли продукт в недельный счёт.
pub fn counts(food: &Food) -> bool {
    food.is_egg == Some(true)
}

/// Открыта ли неделя яиц.
pub fn unlocked() -> bool {
    app_flags::get_bool(EGG_UNLOCKED_KEY)
}

/// День, с которого началась тема.
fn week_open_date() -> Option<NaiveDate> {
    app_flags::get(EGG_WEEK_OPEN_KEY)
        .and_then(|s| NaiveDate::parse_from_str(&s, "%Y-%m-%d").ok())
}

/// Неделя яиц, в которую попадает `today`. Своя сетка, от дня открытия — как у
/// железа, жиров и мяса.
pub fn week_bounds(today: NaiveDate) -> Option<(NaiveDate, NaiveDate)> {
    let open = week_open_date()?;
    if today < open {
        return None;
    }
    let elapsed = (today - open).num_days();
    let start = open + Duration::days(elapsed / 7 * 7);
    Some((start, start + Duration::days(6)))
}

// ── Замер ────────────────────────────────────────────────────────────────────

/// Яйца за день, в штуках.
pub async fn eggs_on(date: &str) -> f64 {
    eggs_from_protein(local::tag_protein_g_on(date, counts).await)
}

async fn eggs_between(from: NaiveDate, to: NaiveDate) -> f64 {
    let mut total = 0.0;
    let mut d = from;
    while d <= to {
        total += eggs_on(&d.format("%Y-%m-%d").to_string()).await;
        d += Duration::days(1);
    }
    total
}

/// Ход текущей недели — для шкалы в виджете.
#[derive(Clone)]
pub struct WeeklyEggs {
    /// Съедено за неделю, штук.
    pub eggs: f64,
    /// Планка — семь штук.
    pub target: f64,
    /// 1…7 — какой сегодня день недели яиц.
    pub day_of_week: u32,
}

impl WeeklyEggs {
    /// Цвет шкалы. Смысл ПРЯМОЙ: чем полнее, тем лучше.
    ///
    /// * зелёная — планка набрана;
    /// * оранжевая — человек отстаёт от равномерного темпа и при таком темпе к концу
    ///   недели не наберёт (на третий день съедено меньше трёх седьмых);
    /// * иначе идёт в темпе — тоже зелёная: ругать за то, что неделя ещё не кончилась,
    ///   не за что.
    ///
    /// Красной здесь нет вовсе: пока неделя идёт, недобор — это не провал, а
    /// незаконченное дело. Итог подводит недельный индикатор.
    pub fn state(&self) -> super::indicators::IndicatorState {
        use super::indicators::IndicatorState;
        if self.eggs >= self.target {
            return IndicatorState::Green;
        }
        let pace = self.target * f64::from(self.day_of_week.clamp(1, 7)) / 7.0;
        if self.eggs < pace {
            IndicatorState::Orange
        } else {
            IndicatorState::Green
        }
    }
}

pub async fn weekly_progress() -> Option<WeeklyEggs> {
    let today = local::today_date();
    let (start, _end) = week_bounds(today)?;
    Some(WeeklyEggs {
        eggs: eggs_between(start, today).await,
        target: WEEKLY_MIN_EGGS,
        day_of_week: (today - start).num_days() as u32 + 1,
    })
}

/// Цвет индикатора по ЗАВЕРШЁННЫМ неделям — общее недельное правило, то же, что у
/// железа, гема, омега-3 и мяса. Неделя закрыта, если планка НАБРАНА.
///
/// Неделя, в которую человек не вёл дневник, не судится: её просто не было.
pub async fn indicator_state() -> super::indicators::IndicatorState {
    use super::indicators::IndicatorState;
    let today = local::today_date();
    let Some((cur_start, _)) = week_bounds(today) else {
        return IndicatorState::Unknown;
    };

    let diary_days: std::collections::HashSet<String> =
        local::list_diary_dates().await.into_iter().collect();
    let mut history: Vec<bool> = Vec::new();
    let mut s = cur_start;
    for _ in 0..super::indicators::WEEKLY_WINDOW {
        s -= Duration::days(7);
        let e = s + Duration::days(6);
        let logged = (0..7).any(|d| {
            diary_days.contains(&(s + Duration::days(d)).format("%Y-%m-%d").to_string())
        });
        if !logged {
            continue;
        }
        history.push(eggs_between(s, e).await >= WEEKLY_MIN_EGGS);
    }
    history.reverse();
    super::indicators::weekly_state(&history)
}

/// Столбики за последние завершённые недели — как у мяса и гема.
pub async fn weekly_series() -> super::indicators::IndicatorSeries {
    let today = local::today_date();
    let mut points: Vec<(String, f64, Option<f64>)> = Vec::new();
    let mut labels: Vec<String> = Vec::new();
    let mut met: Vec<Option<bool>> = Vec::new();
    if let Some((cur_start, _)) = week_bounds(today) {
        let diary_days: std::collections::HashSet<String> =
            local::list_diary_dates().await.into_iter().collect();
        let window = super::indicators::WEEKLY_WINDOW as i64;
        for back in (1..=window).rev() {
            let s = cur_start - Duration::days(7 * back);
            let e = s + Duration::days(6);
            let logged = (0..7).any(|d| {
                diary_days.contains(&(s + Duration::days(d)).format("%Y-%m-%d").to_string())
            });
            let eggs = eggs_between(s, e).await;
            let ratio = logged.then(|| eggs / WEEKLY_MIN_EGGS);
            points.push((s.format("%Y-%m-%d").to_string(), eggs, ratio));
            labels.push(format!("−{back}"));
            // Закрыта — значит НАБРАЛ: доля не меньше единицы.
            met.push(ratio.map(|r| r >= 1.0));
        }
    }
    let missed = met.iter().filter(|m| **m == Some(false)).count() as u32;
    super::indicators::IndicatorSeries {
        key: "egg",
        state: indicator_state().await,
        days: points,
        met_days: met,
        missed,
        labels,
    }
}

/// Закрыта ли хотя бы одна ЗАВЕРШЁННАЯ неделя яиц с открытия темы.
///
/// Следующей главы за яйцами пока нет — условие заведено ради счётчика в виджете,
/// как у мяса до появления этой главы.
pub async fn week_closed_since_open() -> bool {
    let today = local::today_date();
    let (Some(open), Some((cur_start, _))) = (week_open_date(), week_bounds(today)) else {
        return false;
    };
    let diary_days: std::collections::HashSet<String> =
        local::list_diary_dates().await.into_iter().collect();
    let mut s = open;
    while s < cur_start {
        let e = s + Duration::days(6);
        let logged = (0..7).any(|d| {
            diary_days.contains(&(s + Duration::days(d)).format("%Y-%m-%d").to_string())
        });
        if logged && eggs_between(s, e).await >= WEEKLY_MIN_EGGS {
            return true;
        }
        s += Duration::days(7);
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn planka_v_belke_eto_sem_yaic() {
        assert!((weekly_min_protein_g() - 44.1).abs() < 1e-9);
    }

    #[test]
    fn belok_perevoditsya_v_shtuki() {
        // Яичница из двух яиц — 12.6 г белка.
        assert!((eggs_from_protein(12.6) - 2.0).abs() < 1e-9);
    }

    #[test]
    fn nabrannaya_planka_zelenaya_v_lyuboy_den() {
        let w = WeeklyEggs { eggs: 7.0, target: WEEKLY_MIN_EGGS, day_of_week: 2 };
        assert_eq!(w.state(), crate::services::indicators::IndicatorState::Green);
    }

    #[test]
    fn otstavanie_ot_tempa_oranzhevoe() {
        // Четвёртый день: в темпе было бы четыре яйца, съедено одно.
        let w = WeeklyEggs { eggs: 1.0, target: WEEKLY_MIN_EGGS, day_of_week: 4 };
        assert_eq!(w.state(), crate::services::indicators::IndicatorState::Orange);
    }

    #[test]
    fn v_tempe_zelenoe() {
        let w = WeeklyEggs { eggs: 4.0, target: WEEKLY_MIN_EGGS, day_of_week: 4 };
        assert_eq!(w.state(), crate::services::indicators::IndicatorState::Green);
    }
}

//! Гемовое железо — недельный индикатор ПОРЦИЙ, а не миллиграммов.
//!
//! Зачем отдельно от [`super::iron`], который и так считает усвоенное железо: тот
//! отвечает на вопрос «хватило ли», а этот — «из чего». Печень, красное мясо и
//! моллюски дают не одно лишь железо: с ними приходят B12, цинк, селен, холин и
//! полноценный белок, а их усвоение из растительной пищи заметно хуже. Человек,
//! набравший недельную норму железа одной чечевицей, формально закрыл железо и при
//! этом не съел ничего из этого списка.
//!
//! **Порция = 25 г белка из такого продукта.** Мера в белке, а не в граммах
//! продукта, потому что граммы несопоставимы: 100 г печени и 100 г бульона с
//! мясом — разные вещи, а 25 г белка и там и там означают сопоставимое количество
//! собственно мяса. Заодно это само собой учитывает уварку: в готовом продукте
//! белка на 100 г больше, и порция набирается меньшим весом.
//!
//! Неделя и момент открытия — ОБЩИЕ с железом: индикатор появляется вместе с ним и
//! считает те же семь дней, отсчитанные от дня, когда открылась история про железо.
//! Своей сетки недель у него нет намеренно — два индикатора об одном и том же не
//! должны расходиться в границах недель.

use chrono::{Duration, NaiveDate};

use super::{iron, local};

/// Сколько белка из подходящего продукта составляет ОДНУ порцию.
pub const PORTION_PROTEIN_G: f64 = 25.0;

/// Недельная цель в порциях.
pub const WEEKLY_PORTIONS: f64 = 3.0;

/// Виден ли индикатор. Открывается ровно вместе с железом — отдельного условия
/// нет и быть не должно: это две стороны одного разговора.
pub fn unlocked() -> bool {
    iron::unlocked()
}

/// Границы недели — те же, что у железа.
pub fn week_bounds(today: NaiveDate) -> Option<(NaiveDate, NaiveDate)> {
    iron::week_bounds(today)
}

/// Порции за день.
///
/// Считается белок, пришедший ИЗ подходящих продуктов, и делится на размер порции.
/// Блюда раскрываются по составу: в тушёной картошке с мясом засчитывается мясо,
/// а не всё блюдо. Этим занимается общий механизм [`local::tag_protein_g_on`] —
/// тот же, которым считаются яйца и красное мясо.
pub async fn portions_on(date: &str) -> f64 {
    local::tag_protein_g_on(date, |f| f.is_heme == Some(true)).await / PORTION_PROTEIN_G
}

async fn portions_between(from: NaiveDate, to: NaiveDate) -> f64 {
    let mut total = 0.0;
    let mut d = from;
    while d <= to {
        total += portions_on(&d.format("%Y-%m-%d").to_string()).await;
        d += Duration::days(1);
    }
    total
}

/// Ход текущей недели — для шкалы в виджете.
#[derive(Clone)]
pub struct WeeklyHeme {
    pub portions: f64,
    pub target: f64,
    /// 1…7 — какой сегодня день недели гема.
    pub day_of_week: u32,
}

pub async fn weekly_progress() -> Option<WeeklyHeme> {
    let today = local::today_date();
    let (start, _end) = week_bounds(today)?;
    Some(WeeklyHeme {
        portions: portions_between(start, today).await,
        target: WEEKLY_PORTIONS,
        day_of_week: (today - start).num_days() as u32 + 1,
    })
}

/// Цвет индикатора. Меряется своё — порции, — но вердикт выносит ОБЩЕЕ недельное
/// правило, то же, что у железа, омега-3, яиц и красного мяса.
///
/// Неделя, в которую человек не вёл дневник, не судится: её просто не было. Тот же
/// признак, что у железа, — записи в дневнике.
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
        history.push(portions_between(s, e).await >= WEEKLY_PORTIONS);
    }
    history.reverse();
    super::indicators::weekly_state(&history)
}

/// Столбики за последние завершённые недели — так же, как у железа.
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
            // Неделя без дневника попадает в ряд БЕЗ доли: столбика нет, вердикта
            // нет, но подпись остаётся. Пропускать её нельзя — сетка тогда съезжает
            // и у гема оказывается четыре недели там, где у железа восемь, хотя
            // недели у них одни и те же.
            let p = portions_between(s, e).await;
            let ratio = logged.then(|| p / WEEKLY_PORTIONS);
            points.push((s.format("%Y-%m-%d").to_string(), p, ratio));
            labels.push(format!("−{back}"));
            met.push(ratio.map(|r| r >= 1.0));
        }
    }
    let missed = met.iter().filter(|m| **m == Some(false)).count() as u32;
    super::indicators::IndicatorSeries {
        key: "heme",
        state: indicator_state().await,
        days: points,
        met_days: met,
        missed,
        labels,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn porciya_eto_dvadcat_pyat_gramm_belka() {
        assert_eq!(PORTION_PROTEIN_G, 25.0);
        // 150 г говядины (≈26 г белка) — примерно одна порция.
        assert!((26.0 / PORTION_PROTEIN_G - 1.04).abs() < 1e-9);
    }

    #[test]
    fn nedelnaya_cel_tri_porcii() {
        assert_eq!(WEEKLY_PORTIONS, 3.0);
    }
}

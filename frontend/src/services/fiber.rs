//! Клетчатка — НЕДЕЛЬНАЯ планка граммов, которую надо НАБРАТЬ.
//!
//! Восьмая тема пути, после яиц. Индикатор здесь ОДИН и БЕЗ ШКАЛЫ: клетчатка
//! приходит не порциями, которые считают глазами, а фоном всего съеденного за
//! неделю, и суточная полоска у неё дёргалась бы от одного яблока.
//!
//! **Норма.** Современные рекомендации привязывают клетчатку не к человеку, а к
//! калорийности: 14 г на 1000 ккал (IOM AI, действующие Dietary Guidelines).
//! Логика простая — чем больше человек ест, тем больше у него и растительной
//! основы. Снизу норма ограничена 25 г/сут: это минимум ВОЗ для взрослого, и
//! опускаться ниже него нельзя даже на самой скромной планке.
//!
//! Отсюда и недельная величина: суточная норма × 7. Неделя, а не день, потому что
//! клетчатка терпит неравномерность — важно, сколько её набралось в сумме.
//!
//! **Откуда граммы.** Из поля «Клетчатка» в нутриентах продукта — того же, что
//! заполняет фоновый разбор. Ни признака, ни отдельного запроса тут не нужно.

use chrono::{Duration, NaiveDate};

use super::{app_flags, local};

/// App-flag: неделя клетчатки открыта (индикатор виден).
pub const FIBER_UNLOCKED_KEY: &str = "fiber_week_unlocked";

/// App-flag: день, от которого катится сетка недель клетчатки.
pub const FIBER_WEEK_OPEN_KEY: &str = "fiber_week_opened_at";

/// Граммов клетчатки на 1000 ккал рациона — IOM AI, те же 14 г в действующих
/// Dietary Guidelines.
pub const G_PER_1000_KCAL: f64 = 14.0;

/// Нижняя граница суточной нормы, г. Минимум ВОЗ для взрослого: ниже не опускаемся,
/// какой бы скромной ни была калорийная планка.
pub const MIN_G_PER_DAY: f64 = 25.0;

/// Суточная норма от калорийной планки. Без планки — минимум ВОЗ: выдумывать
/// калорийность, чтобы посчитать от неё клетчатку, нечестно.
pub fn daily_target_g(planka_kcal: Option<f64>) -> f64 {
    let from_kcal = planka_kcal.unwrap_or(0.0) / 1000.0 * G_PER_1000_KCAL;
    from_kcal.max(MIN_G_PER_DAY)
}

/// Действующая СУТОЧНАЯ норма: кураторская, если он её задал, иначе наша от
/// калорийной планки.
pub async fn daily_target_effective_g() -> f64 {
    crate::services::curator_plankas::or_ours(
        "fiber",
        daily_target_g(local::calorie_goal_amount().await),
    )
}

/// Недельная планка этого человека ПО СЕГОДНЯШНЕЙ калорийной планке, г.
///
/// Годится для текущей недели; прошлые судятся своей планкой — см.
/// [`weekly_target_on`].
pub async fn weekly_target_g() -> f64 {
    daily_target_effective_g().await * 7.0
}

/// Недельная планка ТОЙ НЕДЕЛИ, что началась `week_start`, г.
///
/// ЗАДНИМ ЧИСЛОМ НИЧЕГО НЕ КРАСНЕЕТ. Норма клетчатки выводится из калорийной, а та
/// пересчитывается каждую неделю: считай мы всю историю по сегодняшнему числу,
/// поднятая планка перекрасила бы в красный недели, которые человек тогда закрыл.
/// Поэтому берётся планка, ДЕЙСТВОВАВШАЯ в первый день той недели, — из того же
/// журнала, по которому судит себя индикатор калорий.
///
/// Журнала может не быть у тех, кто получил планку до его появления: тогда падаем
/// на сегодняшнюю — это лучше, чем судить их по минимуму ВОЗ.
async fn weekly_target_on(week_start: NaiveDate) -> f64 {
    let day = week_start.format("%Y-%m-%d").to_string();
    let planka = match local::planka_on(local::PLANKA_CALORIES, &day).await {
        Some(v) => Some(v),
        None => local::calorie_goal_amount().await,
    };
    daily_target_g(planka) * 7.0
}

/// Открыта ли неделя клетчатки.
pub fn unlocked() -> bool {
    app_flags::get_bool(FIBER_UNLOCKED_KEY)
}

/// День, с которого началась тема.
fn week_open_date() -> Option<NaiveDate> {
    app_flags::get(FIBER_WEEK_OPEN_KEY)
        .and_then(|s| NaiveDate::parse_from_str(&s, "%Y-%m-%d").ok())
}

/// Неделя клетчатки, в которую попадает `today`. Своя сетка, от дня открытия — как
/// у железа, жиров, мяса и яиц.
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

/// Клетчатка за день, г.
pub async fn grams_on(date: &str) -> f64 {
    local::nutrient_grams_on(date, super::indicators::N_FIBER).await
}

async fn grams_between(from: NaiveDate, to: NaiveDate) -> f64 {
    let mut total = 0.0;
    let mut d = from;
    while d <= to {
        total += grams_on(&d.format("%Y-%m-%d").to_string()).await;
        d += Duration::days(1);
    }
    total
}

/// Цвет индикатора по ЗАВЕРШЁННЫМ неделям — общее недельное правило, то же, что у
/// железа, гема, омега-3, мяса и яиц. Неделя закрыта, если планка НАБРАНА.
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
        history.push(grams_between(s, e).await >= weekly_target_on(s).await);
    }
    history.reverse();
    super::indicators::weekly_state(&history)
}

/// Столбики за последние завершённые недели — как у мяса, гема и яиц.
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
            let grams = grams_between(s, e).await;
            let target = weekly_target_on(s).await;
            let ratio = logged.then(|| grams / target);
            points.push((s.format("%Y-%m-%d").to_string(), grams, ratio));
            labels.push(format!("−{back}"));
            // Закрыта — значит НАБРАЛ: доля не меньше единицы.
            met.push(ratio.map(|r| r >= 1.0));
        }
    }
    let missed = met.iter().filter(|m| **m == Some(false)).count() as u32;
    super::indicators::IndicatorSeries {
        key: "fiber",
        state: indicator_state().await,
        days: points,
        met_days: met,
        missed,
        labels,
    }
}

/// Ход ТЕКУЩЕЙ недели — для подписи задания в виджете. Шкалы у клетчатки нет, и
/// эти числа нужны только тексту: сколько набрано и сколько дней осталось.
#[derive(Clone)]
pub struct WeeklyFiber {
    /// Набрано с начала недели, г.
    pub grams: f64,
    /// Недельная планка, г.
    pub target: f64,
    /// 1…7 — какой сегодня день недели клетчатки.
    pub day_of_week: u32,
}

pub async fn weekly_progress() -> Option<WeeklyFiber> {
    let today = local::today_date();
    let (start, _end) = week_bounds(today)?;
    Some(WeeklyFiber {
        grams: grams_between(start, today).await,
        // И текущая неделя тоже — своей планкой: пересчёт калорий среди недели не
        // должен менять задание, которое человек уже читал в понедельник.
        target: weekly_target_on(start).await,
        day_of_week: (today - start).num_days() as u32 + 1,
    })
}

/// Закрыта ли хотя бы одна ЗАВЕРШЁННАЯ неделя клетчатки с открытия темы.
///
/// Следующей главы за клетчаткой пока нет — условие заведено ради счётчика в
/// виджете, как было у мяса и яиц до появления их продолжений.
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
        if logged && grams_between(s, e).await >= weekly_target_on(s).await {
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
    fn norma_rastyot_vmeste_s_planko() {
        // 2600 ккал → 36.4 г/сут.
        assert!((daily_target_g(Some(2600.0)) - 36.4).abs() < 1e-9);
        // 3500 ккал → 49 г/сут.
        assert!((daily_target_g(Some(3500.0)) - 49.0).abs() < 1e-9);
    }

    #[test]
    fn nizhe_minimuma_vo_z_ne_opuskaemsya() {
        // 1500 ккал дали бы 21 г — но ВОЗ говорит не меньше 25.
        assert!((daily_target_g(Some(1500.0)) - MIN_G_PER_DAY).abs() < 1e-9);
        // Планки ещё нет — тоже минимум, а не ноль.
        assert!((daily_target_g(None) - MIN_G_PER_DAY).abs() < 1e-9);
    }

    #[test]
    fn nedelnaya_planka_eto_sem_sutochnyh() {
        assert!((daily_target_g(Some(2600.0)) * 7.0 - 254.8).abs() < 1e-9);
    }
}

//! Iron — a self-contained mechanism, deliberately kept apart from the general
//! nutrient machinery.
//!
//! Why separate: every other tracked nutrient is a plain amount ("eat N mg"), so a
//! single number in `Food.nutrients` says everything. Iron does not work that way —
//! the same milligrams are worth several times more from liver than from lentils,
//! because heme iron is absorbed at ~15–35 % and non-heme iron at ~2–20 %. So iron
//! carries TWO numbers per food (`Food.iron_mg` + `Food.iron_absorption`, filled by
//! its own AI pass, see [`enrich_iron`]) and is judged in ABSORBED milligrams.
//!
//! Consequently iron appears nowhere in the nutrient forms, badges or amount maps.
//! It surfaces in exactly two places, both weekly:
//!   • the weekly gauge in the dashboard widget, and
//!   • the weekly iron indicator,
//! whose week starts on the day the iron story opened (see [`week_bounds`]).

use api_types::Food;
use chrono::{Duration, NaiveDate};

use super::profile;
use super::{ai, app_flags, local};

/// App-flag: the iron week (weekly gauge + weekly indicator) has been unlocked.
pub const IRON_UNLOCKED_KEY: &str = "iron_week_unlocked";
/// App-flag holding the date (YYYY-MM-DD) the iron week opened. It is also the
/// FIRST DAY of every iron week — the weekly window rolls in 7-day steps from here,
/// not from Monday.
pub const IRON_WEEK_OPEN_KEY: &str = "iron_week_opened_at";

/// Whether the iron week is unlocked (weekly gauge + indicator visible).
pub fn unlocked() -> bool {
    app_flags::get_bool(IRON_UNLOCKED_KEY)
}

/// The day the iron week opened, if it has.
pub fn week_open_date() -> Option<NaiveDate> {
    app_flags::get(IRON_WEEK_OPEN_KEY)
        .and_then(|s| NaiveDate::parse_from_str(&s, "%Y-%m-%d").ok())
}

// ── The target ───────────────────────────────────────────────────────────────
// Таблица RDA, поправка на усвоение и разговор про EAR у менструирующих женщин
// переехали в общий крейт `plankas` — в одно место со всеми двенадцатью нормами,
// откуда их считает и кураторское приложение. Здесь остаётся счёт усвоенного
// железа по еде.
pub use plankas::defaults::{
    intake_basis_mg_per_day, rda_mg_per_day, weekly_absorbed_target_mg, RDA_BIOAVAILABILITY,
};

/// То же суточное число, что стоит за планкой — для пояснения «?» на индикаторе.
/// Показывать там RDA было бы враньём: планка построена не от него.
pub fn intake_basis_for_profile() -> f64 {
    intake_basis_mg_per_day(profile::get_sex(), profile::get_age_years())
}

/// Действующая недельная планка усвоенного железа — наша от профиля, пока куратор
/// не назвал свою.
pub fn weekly_target_mg() -> f64 {
    use crate::services::plankas;
    plankas::constant(plankas::Kind::Iron)
}

// ── The week window ──────────────────────────────────────────────────────────

/// The iron week containing `today`, as `(first_day, last_day)` inclusive. Weeks
/// run in 7-day steps from the day the iron story opened, so day 1 of every week is
/// the same weekday the user started on. `None` until the iron week is open.
pub fn week_bounds(today: NaiveDate) -> Option<(NaiveDate, NaiveDate)> {
    let open = week_open_date()?;
    if today < open {
        return None;
    }
    let elapsed = (today - open).num_days();
    let start = open + Duration::days(elapsed / 7 * 7);
    Some((start, start + Duration::days(6)))
}

// ── Measurement ──────────────────────────────────────────────────────────────

/// Absorbed iron (mg) from one day's diary. Foods whose iron pass hasn't run yet
/// contribute nothing — they are not guessed at.
pub async fn absorbed_on(date: &str) -> f64 {
    let entries = local::list_diary(date).await;
    let foods = local::foods_by_ids(entries.iter().map(|e| e.food_id.clone())).await;
    entries
        .iter()
        .filter_map(|e| {
            let f = foods.get(&e.food_id)?;
            let per100 = f.absorbed_iron_mg_per_100g()?;
            let eaten = (e.grams - e.waste_grams).max(0.0);
            Some(per100 * eaten / 100.0)
        })
        .sum()
}

/// Absorbed iron (mg) accumulated over `from..=to`, inclusive. Derived from the
/// diary and the foods on every call — never stored — so a food that gets its iron
/// filled in later changes every range it appears in, with nothing to invalidate.
pub async fn absorbed_between(from: NaiveDate, to: NaiveDate) -> f64 {
    let mut total = 0.0;
    let mut d = from;
    while d <= to {
        total += absorbed_on(&d.format("%Y-%m-%d").to_string()).await;
        d += Duration::days(1);
    }
    total
}

/// What the weekly gauge shows: absorbed mg so far in the CURRENT iron week, its
/// target, and which day of the week we're on. `None` until the week is open.
#[derive(Clone)]
pub struct WeeklyIron {
    pub absorbed_mg: f64,
    pub target_mg: f64,
    /// 1…7 — how far into the current iron week today is.
    pub day_of_week: u32,
}

pub async fn weekly_progress() -> Option<WeeklyIron> {
    let today = local::today_date();
    let (start, _end) = week_bounds(today)?;
    Some(WeeklyIron {
        absorbed_mg: absorbed_between(start, today).await,
        target_mg: weekly_target_mg(),
        day_of_week: (today - start).num_days() as u32 + 1,
    })
}

// ── The weekly indicator ─────────────────────────────────────────────────────

/// Iron indicator colour. Iron only MEASURES differently (absorbed mg, weeks cut
/// from the day its story opened rather than Mon–Sun); the verdict itself comes from
/// the shared weekly rule [`super::indicators::weekly_state`], over the last 8
/// COMPLETED iron weeks. Grey until the first iron week has finished.
pub async fn indicator_state() -> super::indicators::IndicatorState {
    use super::indicators::IndicatorState;
    let today = local::today_date();
    let Some((cur_start, _)) = week_bounds(today) else {
        return IndicatorState::Unknown;
    };
    let target = weekly_target_mg();
    if target <= 0.0 {
        return IndicatorState::Unknown;
    }

    // The last `WEEKLY_WINDOW` COMPLETED iron weeks, walking BACKWARDS from the
    // current one. The grid is anchored to the day the iron story opened (that is
    // what makes day 1 of every week the same weekday), but the history is NOT cut
    // off there: weeks before the story opened are ordinary weeks of the diary and
    // are judged as soon as their iron is known.
    //
    // Every completed week is counted AS IS — including weeks whose food has not been
    // through the iron pass yet. Nothing here is a stored verdict: the sum is derived
    // from the diary and the foods on every read, so the moment the background pass
    // fills a food in, the weeks that food appears in are judged anew. Freezing
    // applies to what the USER entered, never to what the app can still find out.
    // Окно — восемь недель, но недоступные недели В НЕГО НЕ ВХОДЯТ: берём столько,
    // сколько есть. Неделя, в которую человек ещё не вёл дневник, — не проваленная,
    // её просто не было. Раньше цикл шёл на восемь шагов безусловно, и у человека с
    // четырьмя неделями дневника четыре несуществующие недели считались незакрытыми:
    // ровно половина окна — индикатор красный при всех закрытых неделях.
    //
    // Признак — записи в дневнике, а не наличие железа: неделя, в которую человек ел,
    // но железа не набрал, судится как незакрытая (и это верно).
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
        history.push(absorbed_between(s, e).await >= target);
    }
    history.reverse();

    // Вердикт выносит ОБЩЕЕ недельное правило — то же, что у омега-3, яиц и
    // красного мяса. Своей копии этого правила у железа быть не должно.
    super::indicators::weekly_state(&history)
}

/// Закрыта ли ПОСЛЕДНЯЯ завершённая неделя железа, и закончилась ли она не раньше
/// `not_before` — гейт, открывающий жиры.
///
/// Именно последняя, а не «хоть одна за всю историю»: гейт означает «человек держит
/// планку сейчас», а не «однажды получилось». Неделя без дневника не судится — её не
/// было, и провалом она быть не может.
///
/// `not_before` — якорь: день, начиная с которого недели засчитываются (см.
/// `fats::FAT_GATE_ANCHOR_KEY`). Он и делает правило честным: закрыть планку и
/// дождаться конца недели, а не получить открытие за уже прожитое.
pub async fn planka_closed(not_before: NaiveDate) -> bool {
    let today = local::today_date();
    let Some((cur_start, _)) = week_bounds(today) else {
        return false;
    };
    let target = weekly_target_mg();
    if target <= 0.0 {
        return false;
    }
    let s = cur_start - Duration::days(7);
    let e = s + Duration::days(6);
    // Неделя, ЗАКОНЧИВШАЯСЯ до якоря, не считается. Иначе правило «закрой планку и
    // дождись конца недели» превращается в «когда-то закрывал»: у человека с уже
    // закрытой прошлой неделей дверь открылась бы в секунду обновления.
    if e < not_before {
        return false;
    }
    let diary_days: std::collections::HashSet<String> =
        local::list_diary_dates().await.into_iter().collect();
    let logged = (0..7)
        .any(|d| diary_days.contains(&(s + Duration::days(d)).format("%Y-%m-%d").to_string()));
    if !logged {
        return false;
    }
    absorbed_between(s, e).await >= target
}

/// Недельные столбики для гистограммы: последние 8 ЗАВЕРШЁННЫХ недель железа,
/// от старой к свежей, подписанные «−8 … −1» — сколько недель назад. Значение —
/// усвоенные мг за неделю, доля — к недельной норме, поэтому закрытая неделя
/// красится зелёным по общему правилу столбиков.
pub async fn weekly_series() -> super::indicators::IndicatorSeries {
    let today = local::today_date();
    let target = weekly_target_mg();
    let mut points = Vec::new();
    let mut labels = Vec::new();
    let mut met = Vec::new();
    if let Some((cur_start, _)) = week_bounds(today) {
        // Неделя, в которую человек ещё не вёл дневник, не судится — её просто не
        // было. Тот же признак, что и у самого индикатора: записи в дневнике, а не
        // наличие железа. Без этого столбик рисовался нулевым и считался незакрытой
        // неделей, а подсказка сообщала «четыре недели не закрыты» про недели, в
        // которые приложением ещё не пользовались.
        let diary_days: std::collections::HashSet<String> =
            local::list_diary_dates().await.into_iter().collect();
        let window = super::indicators::WEEKLY_WINDOW as i64;
        for back in (1..=window).rev() {
            let s = cur_start - Duration::days(7 * back);
            let e = s + Duration::days(6);
            let logged = (0..7).any(|d| {
                diary_days.contains(&(s + Duration::days(d)).format("%Y-%m-%d").to_string())
            });
            let sum = absorbed_between(s, e).await;
            let ratio = (target > 0.0 && logged).then(|| sum / target);
            points.push((s.format("%Y-%m-%d").to_string(), sum, ratio));
            labels.push(format!("−{back}"));
            met.push(ratio.map(|r| r >= 1.0));
        }
    }
    let missed = met.iter().filter(|m| **m == Some(false)).count() as u32;
    super::indicators::IndicatorSeries {
        key: "iron",
        state: indicator_state().await,
        days: points,
        met_days: met,
        missed,
        labels,
    }
}

// ── The dedicated enrichment pass ────────────────────────────────────────────

/// True while this food's iron is still unknown.
pub fn needs_iron(food: &Food) -> bool {
    food.iron_mg.is_none() || food.iron_absorption.is_none()
}

/// Fill a food's iron (amount + absorbed fraction) with ONE focused AI request.
/// FAILS LOUDLY — the caller's retry/error-log wrapper decides what to do.
/// `identity` — готовое опознание из конвейера признаков; пустая строка допустима.
pub async fn enrich_iron(food: &Food, identity: &str) -> Result<(), String> {
    let (mg, absorption) = super::iron_pipeline::lookup_iron(&food.name, identity).await?;
    local::cache_food_iron(&food.id, mg, absorption).await;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn absorbed_iron_needs_both_numbers() {
        let mut f = api_types::Food {
            id: "x".into(), name: "x".into(), kcal: 0.0, protein: 0.0, fat: 0.0, carbs: 0.0,
            nutrients: Default::default(), package_weight: None, is_recipe: false, recipe_id: None,
            keywords: Vec::new(),
            archived: false, is_restaurant: false,
            is_veg_fruit: None, is_heme: None,
            is_milk_globule: None,
            is_red_meat: None,
            is_processed_meat: None,
            is_egg: None,
            iron_mg: None, iron_absorption: None, fat_profile: None,
            balance_fat_profile: None,
            created_at: String::new(), updated_at: String::new(),
        };
        assert_eq!(f.absorbed_iron_mg_per_100g(), None);
        f.iron_mg = Some(9.0);
        assert_eq!(f.absorbed_iron_mg_per_100g(), None, "одного количества мало");
        f.iron_absorption = Some(0.25);
        assert_eq!(f.absorbed_iron_mg_per_100g(), Some(2.25));
        // Печень против чечевицы: одинаковые миллиграммы — разная польза.
        f.iron_absorption = Some(0.05);
        let lentils = f.absorbed_iron_mg_per_100g().unwrap();
        assert!((lentils - 0.45).abs() < 1e-9, "{lentils}");
        // Бессмысленный коэффициент не принимается.
        f.iron_absorption = Some(1.5);
        assert_eq!(f.absorbed_iron_mg_per_100g(), None);
    }
}

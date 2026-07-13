//! Nutrition indicators: turn a week (and up to 8 weeks of history) of diary data
//! into a green / orange / red / unknown state per indicator.
//!
//! Two families (per the product spec):
//!
//! * **Daily-goal** (calcium, iron, fiber, veg/fruit): over the LAST 7 DAYS, count
//!   the days the per-day target was missed.
//!     0 misses → green · 1–3 → orange · ≥4 → red.
//!
//! * **Weekly-goal** (omega-3, eggs, red/processed meat): the rolling last-7-days
//!   sum vs a weekly target decides orange/green for THIS week; the history of
//!   complete Mon–Sun weeks (up to the last 8 = ~2 months, only weeks that have any
//!   diary data) decides red: if the goal was MISSED in > 50 % of those weeks it's a
//!   chronic problem → red. Red takes precedence over orange.
//!   "Missed" for a LIMIT goal (red meat) means the amount went OVER the limit.
//!
//! `Unknown` (grey) is used when a nutrient has no data at all yet (e.g. calcium is
//! never present on any logged food until the nutrient-fill pipeline exists).

use std::collections::{HashMap, HashSet};

use chrono::{Datelike, Duration, NaiveDate};

use super::local;
use super::profile::{self, Sex};

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum IndicatorState {
    Green,
    Orange,
    Red,
    Unknown,
}

// ── Targets (WHO / user-set; adjustable) ─────────────────────────────────────
const FIBER_PER_DAY_G: f64 = 25.0; // WHO ≥25 g/day
const CALCIUM_PER_DAY_MG: f64 = 1000.0; // user: 1 g/day for everyone
const EGG_PER_WEEK_G: f64 = 350.0; // ~1 egg/day (≈50 g × 7)
const OMEGA3_PER_WEEK_MG: f64 = 3500.0; // user: 3.5 g/week
const RED_MEAT_LIMIT_PER_WEEK_G: f64 = 700.0; // user: up to 700 g/week

/// Vegetables/fruit target (g/day): user-set — women 600, men 800. Unknown sex →
/// 600 (the lower, so it isn't spuriously missed before the persona is complete).
fn veg_fruit_per_day_g() -> f64 {
    match profile::get_sex() {
        Some(Sex::Male) => 800.0,
        _ => 600.0,
    }
}

/// Iron target (mg/day): premenopausal women 18, everyone else 8 (WHO/RDA). Unknown
/// sex is treated as the higher (18) — conservative; the row only shows once the
/// persona (incl. sex) is set anyway.
fn iron_per_day_mg() -> f64 {
    match (profile::get_sex(), profile::get_age_years()) {
        (Some(Sex::Female), Some(age)) if age < 51 => 18.0,
        (Some(Sex::Female), None) => 18.0,
        (None, _) => 18.0,
        _ => 8.0,
    }
}

// Nutrient display names. `Food.nutrients` is keyed by the display name (same as
// `goal.nutrient`), so these are used directly as the map keys. The background
// enricher writes under the exact same names.
pub const N_CALCIUM: &str = "Кальций";
pub const N_IRON: &str = "Железо";
pub const N_OMEGA3: &str = "Омега-3";
pub const N_FIBER: &str = "Клетчатка";

// ── Pure state machines (unit-tested) ────────────────────────────────────────

/// Daily-goal colour from the number of missed days out of the last 7.
fn daily_state(misses: u32) -> IndicatorState {
    match misses {
        0 => IndicatorState::Green,
        1..=3 => IndicatorState::Orange,
        _ => IndicatorState::Red,
    }
}

/// Weekly-goal colour. `current_met` = this rolling week hit the goal;
/// `history_met` = per complete-week whether the goal was met (only weeks with data).
fn weekly_state(current_met: bool, history_met: &[bool]) -> IndicatorState {
    if !history_met.is_empty() {
        let missed = history_met.iter().filter(|m| !**m).count();
        // Chronic: missed in MORE THAN 50 % of the assessed weeks.
        if missed * 2 > history_met.len() {
            return IndicatorState::Red;
        }
    }
    if current_met {
        IndicatorState::Green
    } else {
        IndicatorState::Orange
    }
}

// ── Data gathering ───────────────────────────────────────────────────────────

fn fmt(d: NaiveDate) -> String {
    d.format("%Y-%m-%d").to_string()
}

/// Does the user have at least a week of diary history? (The indicators row is
/// hidden before that.)
pub async fn enough_history() -> bool {
    // Count DISTINCT days with entries — `list_diary_dates` returns one date per
    // entry (with duplicates), so 7 items in a single day must NOT pass.
    let days: HashSet<String> = local::list_diary_dates().await.into_iter().collect();
    days.len() >= 7
}

/// Compute all seven indicator states, keyed the same as the widget icons.
pub async fn compute() -> Vec<(&'static str, IndicatorState)> {
    let today = chrono::Local::now().date_naive();
    // 70-day window covers the rolling week + up to 8 complete Mon–Sun weeks.
    let window: Vec<NaiveDate> = (0..70).map(|i| today - Duration::days(i)).collect();
    let diary_days: HashSet<String> = local::list_diary_dates().await.into_iter().collect();

    // Per-metric per-date value maps.
    let veg = gather_veg(&window).await;
    let eggs = gather_egg(&window).await;
    let meat = gather_meat(&window).await;
    let cal = gather_nutrient(&window, N_CALCIUM).await;
    let iron = gather_nutrient(&window, N_IRON).await;
    let fib = gather_nutrient(&window, N_FIBER).await;
    let omega = gather_nutrient(&window, N_OMEGA3).await;

    let last7: Vec<NaiveDate> = window.iter().take(7).copied().collect();

    vec![
        ("calcium", daily_nutrient(&cal, &last7, CALCIUM_PER_DAY_MG)),
        ("omega3", weekly(&omega, &diary_days, today, OMEGA3_PER_WEEK_MG, false, true)),
        ("eggs", weekly(&eggs, &diary_days, today, EGG_PER_WEEK_G, false, false)),
        ("iron", daily_nutrient(&iron, &last7, iron_per_day_mg())),
        ("red_meat", weekly(&meat, &diary_days, today, RED_MEAT_LIMIT_PER_WEEK_G, true, false)),
        ("veg_fruit", daily_classifier(&veg, &last7, veg_fruit_per_day_g())),
        ("fiber", daily_nutrient(&fib, &last7, FIBER_PER_DAY_G)),
    ]
}

// ── Progressive disclosure ───────────────────────────────────────────────────
// Which metrics are currently surfaced in the widget. The product opens more over
// time; today only the week-1 set. Calories is the planka gauge (drawn directly by
// the widget, not via `daily_gauges`).
pub const UNLOCKED_GAUGES: &[&str] = &["protein", "veg_fruit"];
pub const UNLOCKED_INDICATORS: &[&str] = &["protein", "veg_fruit"];

// ── Per-indicator per-day cache ──────────────────────────────────────────────
// Each cacheable indicator has its OWN store (`ind_<key>`), keyed by date, holding
// the completed-day aggregate so it isn't recomputed on every render. Today is
// never cached (it's still changing). Invalidated per-day on diary edits and
// wholesale on food changes (nutrients/tags shift many days at once).

#[derive(Clone, serde::Serialize, serde::Deserialize)]
struct IndDay {
    date: String,
    value: f64,
}

/// Indicator keys that have a per-day cache store. Keep in sync with the `ind_*`
/// object stores in `db::builder` and with [`invalidate_day`]/[`clear_cache`].
const CACHED_STORES: &[&str] = &["ind_protein", "ind_veg_fruit"];

/// The cache store for `key`, or None if the indicator isn't cached.
fn cache_store(key: &str) -> Option<&'static str> {
    match key {
        "protein" => Some("ind_protein"),
        "veg_fruit" => Some("ind_veg_fruit"),
        _ => None,
    }
}

/// Raw per-day aggregate for `key` on `date` — the number compared to the target.
async fn compute_day_value(key: &str, date: &str) -> f64 {
    match key {
        "protein" => local::protein_grams_on(date).await,
        "veg_fruit" => local::veg_fruit_grams_on(date).await,
        "calcium" => local::nutrient_grams_on(date, N_CALCIUM).await,
        "iron" => local::nutrient_grams_on(date, N_IRON).await,
        "fiber" => local::nutrient_grams_on(date, N_FIBER).await,
        _ => 0.0,
    }
}

/// Cached-or-computed per-day value. For a cacheable indicator a hit returns the
/// stored value; a miss computes it and stores it. `date` is expected to be a
/// COMPLETED day — the caller never caches today (it's still changing).
async fn day_value_cached(key: &str, date: &str) -> f64 {
    let Some(store) = cache_store(key) else {
        return compute_day_value(key, date).await;
    };
    if let Some(rec) = crate::services::db::get::<IndDay>(store, date).await {
        return rec.value;
    }
    let value = compute_day_value(key, date).await;
    crate::services::db::put(store, &IndDay { date: date.to_string(), value }).await;
    value
}

/// Drop cached values for `date` across every indicator cache — call when the
/// diary for that day changes.
pub async fn invalidate_day(date: &str) {
    for store in CACHED_STORES {
        crate::services::db::delete(store, date).await;
    }
}

/// Clear every indicator cache — call when a food's nutrients/tags change (a food
/// edit or a background classification), since that can shift many days' values.
pub async fn clear_cache() {
    for store in CACHED_STORES {
        crate::services::db::clear(store).await;
    }
}

/// The daily target for `key` (0 → not computable yet, e.g. protein before the
/// profile/weight is set).
async fn target_for(key: &str) -> f64 {
    match key {
        "protein" => local::list_weight_entries()
            .await
            .into_iter()
            .last()
            .map(|e| profile::protein_target_from_profile(e.weight_kg) as f64)
            .unwrap_or(0.0),
        "veg_fruit" => veg_fruit_per_day_g(),
        "calcium" => CALCIUM_PER_DAY_MG,
        "iron" => iron_per_day_mg(),
        "fiber" => FIBER_PER_DAY_G,
        _ => 0.0,
    }
}

/// Classifier metrics (veg/fruit) always have data → never Unknown. Nutrient
/// metrics can be Unknown (grey) when there's no data in the window.
fn is_classifier(key: &str) -> bool {
    key == "veg_fruit"
}

/// Indicator colour for `key` over the 7 COMPLETED days ending yesterday, read
/// through the per-day cache. Unknown (grey) when the target is unset or a nutrient
/// metric has no data yet.
pub async fn indicator_state(key: &str) -> IndicatorState {
    let target = target_for(key).await;
    if target <= 0.0 {
        return IndicatorState::Unknown;
    }
    let today = chrono::Local::now().date_naive();
    let days: Vec<NaiveDate> = (1..=7).map(|i| today - Duration::days(i)).collect();
    let mut values = Vec::with_capacity(days.len());
    for d in &days {
        values.push(day_value_cached(key, &fmt(*d)).await);
    }
    if !is_classifier(key) && values.iter().sum::<f64>() == 0.0 {
        return IndicatorState::Unknown;
    }
    let misses = values.iter().filter(|v| **v < target).count() as u32;
    daily_state(misses)
}

/// States for the currently-unlocked indicators, in display order (cached).
pub async fn unlocked_indicator_states() -> Vec<(&'static str, IndicatorState)> {
    let mut out = Vec::new();
    for key in UNLOCKED_INDICATORS.iter().copied() {
        out.push((key, indicator_state(key).await));
    }
    out
}

/// One daily gauge: TODAY's amount toward `target`, plus the indicator's state to
/// colour it. `state == Unknown` → grey (no data / target unset yet).
#[derive(Clone)]
pub struct DailyGauge {
    pub key: &'static str,
    pub value: f64, // eaten TODAY, in `unit`
    pub target: f64,
    pub unit: &'static str,
    pub state: IndicatorState,
}

fn unit_for(key: &str) -> &'static str {
    match key {
        "calcium" | "iron" => "мг",
        _ => "г",
    }
}

/// Today's progress toward each UNLOCKED daily target, for the dashboard gauges.
/// The value is TODAY only (live); the colour is the indicator's 7-day state.
pub async fn daily_gauges() -> Vec<DailyGauge> {
    let today = fmt(chrono::Local::now().date_naive());
    let mut out = Vec::new();
    for key in UNLOCKED_GAUGES.iter().copied() {
        out.push(DailyGauge {
            key,
            value: compute_day_value(key, &today).await,
            target: target_for(key).await,
            unit: unit_for(key),
            state: indicator_state(key).await,
        });
    }
    out
}

/// Daily state for a CLASSIFIER metric (data always available → never Unknown).
fn daily_classifier(values: &HashMap<String, f64>, last7: &[NaiveDate], target: f64) -> IndicatorState {
    let misses = last7.iter()
        .filter(|d| *values.get(&fmt(**d)).unwrap_or(&0.0) < target)
        .count() as u32;
    daily_state(misses)
}

/// Daily state for a NUTRIENT metric: Unknown when there's no data in the window.
fn daily_nutrient(values: &HashMap<String, f64>, last7: &[NaiveDate], target: f64) -> IndicatorState {
    let week_total: f64 = last7.iter().map(|d| values.get(&fmt(*d)).copied().unwrap_or(0.0)).sum();
    if week_total == 0.0 {
        return IndicatorState::Unknown;
    }
    let misses = last7.iter()
        .filter(|d| *values.get(&fmt(**d)).unwrap_or(&0.0) < target)
        .count() as u32;
    daily_state(misses)
}

/// Weekly state. `is_limit` = the goal is an upper bound (met = under it).
/// `is_nutrient` = Unknown when there's no data at all in the window.
fn weekly(
    values: &HashMap<String, f64>,
    diary_days: &HashSet<String>,
    today: NaiveDate,
    target: f64,
    is_limit: bool,
    is_nutrient: bool,
) -> IndicatorState {
    let val = |d: NaiveDate| values.get(&fmt(d)).copied().unwrap_or(0.0);
    let met = |sum: f64| if is_limit { sum <= target } else { sum >= target };

    // Rolling current week.
    let cur_sum: f64 = (0..7).map(|i| val(today - Duration::days(i))).sum();

    // Complete Mon–Sun weeks before this week, most recent 8, only with data.
    let this_monday = today - Duration::days(today.weekday().num_days_from_monday() as i64);
    let mut history_met = Vec::new();
    for k in 1..=8i64 {
        let mon = this_monday - Duration::days(7 * k);
        let dates: Vec<NaiveDate> = (0..7).map(|j| mon + Duration::days(j)).collect();
        if !dates.iter().any(|d| diary_days.contains(&fmt(*d))) {
            continue; // skip weeks with no logging
        }
        let sum: f64 = dates.iter().map(|d| val(*d)).sum();
        history_met.push(met(sum));
    }

    if is_nutrient && values.values().sum::<f64>() == 0.0 {
        // No data for this nutrient anywhere in the window yet.
        return IndicatorState::Unknown;
    }

    weekly_state(met(cur_sum), &history_met)
}

async fn gather_veg(window: &[NaiveDate]) -> HashMap<String, f64> {
    let mut m = HashMap::new();
    for d in window {
        let s = fmt(*d);
        m.insert(s.clone(), local::veg_fruit_grams_on(&s).await);
    }
    m
}
async fn gather_egg(window: &[NaiveDate]) -> HashMap<String, f64> {
    let mut m = HashMap::new();
    for d in window {
        let s = fmt(*d);
        m.insert(s.clone(), local::egg_grams_on(&s).await);
    }
    m
}
async fn gather_meat(window: &[NaiveDate]) -> HashMap<String, f64> {
    let mut m = HashMap::new();
    for d in window {
        let s = fmt(*d);
        m.insert(s.clone(), local::red_meat_grams_on(&s).await);
    }
    m
}
async fn gather_nutrient(window: &[NaiveDate], key: &str) -> HashMap<String, f64> {
    let mut m = HashMap::new();
    for d in window {
        let s = fmt(*d);
        m.insert(s.clone(), local::nutrient_grams_on(&s, key).await);
    }
    m
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn daily_bands() {
        assert_eq!(daily_state(0), IndicatorState::Green);
        assert_eq!(daily_state(1), IndicatorState::Orange);
        assert_eq!(daily_state(3), IndicatorState::Orange);
        assert_eq!(daily_state(4), IndicatorState::Red);
        assert_eq!(daily_state(7), IndicatorState::Red);
    }

    #[test]
    fn weekly_current_only() {
        assert_eq!(weekly_state(true, &[]), IndicatorState::Green);
        assert_eq!(weekly_state(false, &[]), IndicatorState::Orange);
    }

    #[test]
    fn weekly_chronic_red_over_half() {
        // 2 of 3 weeks missed → >50% → red, regardless of the current week.
        assert_eq!(weekly_state(true, &[false, false, true]), IndicatorState::Red);
        assert_eq!(weekly_state(false, &[false, false, true]), IndicatorState::Red);
    }

    #[test]
    fn weekly_not_chronic() {
        // 1 of 3 missed → not >50% → current week decides.
        assert_eq!(weekly_state(true, &[true, true, false]), IndicatorState::Green);
        assert_eq!(weekly_state(false, &[true, true, false]), IndicatorState::Orange);
        // exactly 50% (2 of 4) is NOT > 50% → not chronic.
        assert_eq!(weekly_state(true, &[false, false, true, true]), IndicatorState::Green);
    }
}

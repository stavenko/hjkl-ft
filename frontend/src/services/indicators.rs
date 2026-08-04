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

/// The CALORIE indicator's success band: a day is green when intake landed
/// STRICTLY within ±50 kcal of that day's planka (planka 3000 → 2951…3049 is
/// green; 2950/3050 already miss). Indicator/gate semantics ONLY.
const CALORIE_BAND_KCAL: f64 = 50.0;

/// Green-day test for one indicator from its frozen `(value, ratio)` pair.
/// Calories: within the ±band of the day's planka (reconstructed as
/// `value / ratio`). Everything else: AtLeast — ratio ≥ 1.0.
fn day_green(key: &str, value: f64, ratio: Option<f64>) -> bool {
    match key {
        "calories" => match ratio {
            Some(r) if r > 0.0 && value > 0.0 => {
                let target = value / r;
                (value - target).abs() < CALORIE_BAND_KCAL
            }
            _ => false,
        },
        _ => matches!(ratio, Some(r) if r >= 1.0),
    }
}

/// Missed-day test — the judgeable inverse of [`day_green`]: a day counts as a
/// miss only when its frozen ratio exists (the target was known at freeze time).
fn day_missed(key: &str, value: f64, ratio: Option<f64>) -> bool {
    match key {
        "calories" => ratio.is_some() && !day_green(key, value, ratio),
        _ => ratio.map_or(false, |r| r < 1.0),
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
    let today = crate::services::local::today_date();
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
/// The week-2 indicators — ALSO the set the "keep green 7 days" gate watches. The
/// step indicator is NOT here: it's added to the DISPLAY by [`displayed_indicators`]
/// once the activity week unlocks, and gets its OWN separate gate.
/// `calories` is the planka-adherence indicator: green day = intake within
/// ±[`CALORIE_BAND_KCAL`] of that day's planka (indicator/gate semantics only —
/// the gauge and the goal keep their AtMost meaning).
pub const UNLOCKED_INDICATORS: &[&str] = &["calories", "protein", "veg_fruit"];

/// App-flag: the activity week (step planka + step indicator) has been unlocked.
const ACTIVITY_UNLOCKED_KEY: &str = "activity_week_unlocked";

/// App-flag: the calcium week (calcium goal + calcium indicator + gauge) has been
/// unlocked. Opens once the activity (steps) gate is cleared.
const CALCIUM_UNLOCKED_KEY: &str = "calcium_week_unlocked";

/// Whether the activity week is unlocked (step indicator visible).
pub fn activity_unlocked() -> bool {
    crate::services::app_flags::get_bool(ACTIVITY_UNLOCKED_KEY)
}

/// Whether the calcium week is unlocked (calcium indicator + gauge visible).
pub fn calcium_unlocked() -> bool {
    crate::services::app_flags::get_bool(CALCIUM_UNLOCKED_KEY)
}

/// Indicators shown in the widget (icons + histograms), in display order: the
/// week-2 set plus `steps` once the activity week is unlocked. NB: distinct from
/// the gate set (`UNLOCKED_INDICATORS`) — steps has its own gate, so adding it to
/// the display must NOT change the protein/veg-fruit gate.
pub fn displayed_indicators() -> Vec<&'static str> {
    let mut v: Vec<&'static str> = UNLOCKED_INDICATORS.to_vec();
    if activity_unlocked() {
        v.push("steps");
    }
    if calcium_unlocked() {
        v.push("calcium");
    }
    v
}

/// Daily gauges shown on the dashboard, in display order: the week-1 set plus
/// `calcium` once the calcium week is unlocked (steps is NOT a gauge — it has its
/// own widget). Distinct from the gate sets — each unlock has its own gate.
pub fn displayed_gauges() -> Vec<&'static str> {
    let mut v: Vec<&'static str> = UNLOCKED_GAUGES.to_vec();
    if calcium_unlocked() {
        v.push("calcium");
    }
    v
}

/// Unlock the activity week once the week-2 gate (protein + veg-fruit green for a
/// week) is cleared: compute the step planka from the whole history and set it as
/// the daily Steps goal, then flip the flag so the step indicator appears and its
/// own gate begins. Idempotent (guarded by the flag) and a no-op until there is
/// step data to base a planka on. Call on launch and after step/diary saves.
pub async fn maybe_unlock_activity_week() {
    if activity_unlocked() {
        return;
    }
    if green_gate_progress().await < GREEN_GATE_DAYS {
        return; // week-2 gate not cleared yet
    }
    let Some(planka) = local::steps_planka_from_history().await else {
        return; // no step history yet → can't set a planka; wait for data
    };
    crate::services::profile::set_steps_planka(planka as f64);
    // Anchor the step gate at today so "hold steps a week" counts from now.
    let today = crate::services::local::today_date();
    crate::services::app_flags::set(STEPS_GATE_OPEN_KEY, &fmt(today));
    crate::services::app_flags::set_bool(ACTIVITY_UNLOCKED_KEY, true);
}

/// Unlock the calcium week once the activity (steps) gate is cleared: set the daily
/// calcium goal (1 g/day) so the AI starts filling calcium on new foods, flip the
/// flag so the calcium indicator + gauge appear, and anchor the calcium gate at
/// today so its own "keep it green a week" begins now. Idempotent (guarded by the
/// flag). Call on launch, after the activity week is (re)checked.
pub async fn maybe_unlock_calcium_week() {
    if calcium_unlocked() {
        return;
    }
    if !activity_unlocked() {
        return; // the activity week must open (and be gated) first
    }
    if steps_gate_progress().await < GREEN_GATE_DAYS {
        return; // steps gate not cleared yet
    }
    local::set_calcium_goal(CALCIUM_PER_DAY_MG).await;
    let today = crate::services::local::today_date();
    crate::services::app_flags::set(CALCIUM_GATE_OPEN_KEY, &fmt(today));
    crate::services::app_flags::set_bool(CALCIUM_UNLOCKED_KEY, true);
}

// ── Per-indicator per-day cache ──────────────────────────────────────────────
// Each cacheable indicator has its OWN store (`ind_<key>`), keyed by date, holding
// the completed-day aggregate so it isn't recomputed on every render. Today is
// never cached (it's still changing). Invalidated per-day on diary edits and
// wholesale on food changes (nutrients/tags shift many days at once).

#[derive(Clone, serde::Serialize, serde::Deserialize)]
struct IndDay {
    date: String,
    value: f64,
    /// The DEGREE to which the day hit its target: `value / target`, FROZEN with the
    /// target valid at summarization time (1.0 = exactly on target, 0.5 = half, 1.5 =
    /// one-and-a-half). Frozen, not recomputed, when the target later changes. Storing
    /// the ratio (with the value) lets us both show the degree and reconstruct that
    /// day's target (`target = value / ratio`). `None` = a legacy record (or a day
    /// summarized while the target was unknown).
    #[serde(default)]
    ratio: Option<f64>,
    /// RFC3339 freeze moment — the sync conflict key (first computation wins).
    /// Empty on rows frozen before ind-day sync existed.
    #[serde(default)]
    computed_at: String,
}

fn now_stamp() -> String {
    chrono::Utc::now().to_rfc3339()
}

/// Indicator keys that have a per-day cache store. Keep in sync with the `ind_*`
/// object stores in `db::builder` and with [`invalidate_day`]/[`clear_cache`].
const CACHED_STORES: &[&str] = &["ind_protein", "ind_veg_fruit", "ind_steps", "ind_calories"];

/// The cache store for `key`, or None if the indicator isn't cached.
fn cache_store(key: &str) -> Option<&'static str> {
    match key {
        "protein" => Some("ind_protein"),
        "veg_fruit" => Some("ind_veg_fruit"),
        "steps" => Some("ind_steps"),
        "calories" => Some("ind_calories"),
        _ => None,
    }
}

/// Raw per-day aggregate for `key` on `date` — the number compared to the target.
async fn compute_day_value(key: &str, date: &str) -> f64 {
    match key {
        "protein" => local::protein_grams_on(date).await,
        "veg_fruit" => local::veg_fruit_grams_on(date).await,
        "steps" => local::steps_on(date).await,
        "calories" => local::kcal_on(date).await,
        "calcium" => local::nutrient_grams_on(date, N_CALCIUM).await,
        "iron" => local::nutrient_grams_on(date, N_IRON).await,
        "fiber" => local::nutrient_grams_on(date, N_FIBER).await,
        _ => 0.0,
    }
}

/// Cached-or-computed per-day `(value, ratio)`, where `ratio = value / target` is
/// FROZEN at summarization time (`None` while the target is unknown). A hit returns
/// the stored pair; a miss computes the value, freezes the ratio, and stores both.
/// `date` is expected to be a COMPLETED day — the caller never caches today.
///
/// The ratio is frozen deliberately: the target (protein from weight, veg from sex,
/// the calorie planka) can change later, but the degree to which a past day hit its
/// target must not be recomputed retrospectively.
async fn day_cached(key: &str, date: &str) -> (f64, Option<f64>) {
    let Some(store) = cache_store(key) else {
        let value = compute_day_value(key, date).await;
        return (value, ratio_now(key, value).await);
    };
    if let Some(rec) = crate::services::db::get::<IndDay>(store, date).await {
        if rec.ratio.is_some() {
            return (rec.value, rec.ratio);
        }
        // Legacy / not-yet-frozen record → freeze now (if the target is known) and store.
        let ratio = ratio_now(key, rec.value).await;
        if ratio.is_some() {
            crate::services::db::put(
                store,
                &IndDay { date: date.to_string(), value: rec.value, ratio, computed_at: now_stamp() },
            )
            .await;
        }
        return (rec.value, ratio);
    }
    let value = compute_day_value(key, date).await;
    let ratio = ratio_now(key, value).await;
    crate::services::db::put(
        store,
        &IndDay { date: date.to_string(), value, ratio, computed_at: now_stamp() },
    )
    .await;
    (value, ratio)
}

/// `value / target` at the CURRENT target — `None` when the target isn't known yet.
/// Used only to freeze a day's ratio the first time it's summarized.
async fn ratio_now(key: &str, value: f64) -> Option<f64> {
    let target = target_for(key).await;
    (target > 0.0).then(|| value / target)
}

/// Drop cached values for `date` across every indicator cache — call when the
/// diary for that day changes.
pub async fn invalidate_day(date: &str) {
    for store in CACHED_STORES {
        crate::services::db::delete(store, date).await;
    }
}

/// Clear every indicator cache — call for a bulk change (e.g. a range delete) that
/// can touch arbitrary past days.
pub async fn clear_cache() {
    for store in CACHED_STORES {
        crate::services::db::clear(store).await;
    }
}

/// Write-through for the STEP indicator: recompute `date`'s steps ratio against the
/// CURRENT planka and store it in `ind_steps`. Called from `save_steps` — the ONLY
/// moment the step indicator recomputes (steps are one final value per day, so no
/// waiting for day-end like the diary needs). Overwrites any previous row, so
/// re-logging or editing an old day rewrites that day's verdict. A `None` ratio
/// (planka not set yet) is stored but never counts as a miss.
pub async fn record_steps(date: &str) {
    let value = local::steps_on(date).await;
    let ratio = ratio_now("steps", value).await;
    crate::services::db::put(
        "ind_steps",
        &IndDay { date: date.to_string(), value, ratio, computed_at: now_stamp() },
    )
    .await;
}

// ── Sync bridge: frozen indicator days ride the regular sync ────────────────
// A day is computed ONCE (by whichever device had the planka + diary data at
// that moment) and then travels as DATA; other devices apply the ready value
// instead of computing their own. Conflict resolution lives on the server
// (first-writer-wins by `computed_at`).

/// Store name → the indicator key used in the wire `id` (`"<key>:<date>"`).
fn store_indicator(store: &str) -> &'static str {
    match store {
        "ind_protein" => "protein",
        "ind_veg_fruit" => "veg_fruit",
        "ind_steps" => "steps",
        _ => "calories",
    }
}

/// Every locally frozen indicator day, in the sync wire shape.
pub async fn export_ind_days() -> Vec<api_types::IndDayRow> {
    let mut out = Vec::new();
    for store in CACHED_STORES {
        let indicator = store_indicator(store);
        for r in crate::services::db::list_all::<IndDay>(store).await {
            out.push(api_types::IndDayRow {
                id: format!("{indicator}:{}", r.date),
                indicator: indicator.to_string(),
                date: r.date,
                value: r.value,
                ratio: r.ratio,
                computed_at: r.computed_at,
            });
        }
    }
    out
}

/// The local `ind_*` cache store for an indicator key (sync wire mapping).
pub fn store_for_indicator(indicator: &str) -> Option<&'static str> {
    cache_store(indicator)
}

/// Sync-apply of ONE wire indicator-day row: UNTRACKED write (remote data must
/// not re-enter the outbox), skipped entirely when the local row is identical.
pub async fn apply_ind_day(row: &serde_json::Value) {
    let parsed: api_types::IndDayRow = match serde_json::from_value(row.clone()) {
        Ok(p) => p,
        Err(e) => {
            leptos::logging::error!("sync v2: bad ind_days row ({e}): {row}");
            return;
        }
    };
    let Some(store) = cache_store(&parsed.indicator) else {
        leptos::logging::error!("sync v2: unknown indicator {:?}", parsed.indicator);
        return;
    };
    let incoming = IndDay {
        date: parsed.date.clone(),
        value: parsed.value,
        ratio: parsed.ratio,
        computed_at: parsed.computed_at.clone(),
    };
    // Journal order is the truth (per-row conflicts, incl. the first-computation-
    // wins rule for indicator days, are resolved by the sync merge gate before a
    // batch is ever pushed); identical rows are skipped to keep idle syncs silent.
    if let Some(existing) = crate::services::db::get::<IndDay>(store, &parsed.date).await {
        if existing.date == incoming.date
            && existing.value == incoming.value
            && existing.ratio == incoming.ratio
            && existing.computed_at == incoming.computed_at
        {
            return;
        }
    }
    crate::services::db::put_untracked(store, &incoming).await;
}

// ── Calorie planka (per-day, frozen) ─────────────────────────────────────────
// The calorie planka is an AtMost goal recomputed every week. To keep PAST days
// honest (the diary must show the planka that applied THAT day, not today's), we
// freeze each completed day's `(intake, ratio)` — like the other indicators — from
// which that day's planka is reconstructed (`target = intake / ratio`). This is
// entirely separate from the weekly recompute: the recompute is unchanged.

/// The calorie planka that APPLIED on `date`. Today → the live planka. A completed
/// past day → the target frozen when the day was summarized (`intake / ratio`), so
/// it survives the weekly recompute; falls back to the current planka when there is
/// no usable frozen record (a 0-intake day, or days before this cache existed).
pub async fn calorie_planka_on(date: &str) -> Option<f64> {
    let current = local::calorie_goal_amount().await;
    let today = fmt(crate::services::local::today_date());
    if date >= today.as_str() {
        return current; // today / future: the live planka applies
    }
    let (value, ratio) = day_cached("calories", date).await;
    match ratio {
        Some(r) if r > 0.0 => Some(value / r),
        _ => current,
    }
}

/// Freeze the last two weeks of completed days' calorie result at the CURRENT planka
/// (idempotent — already-frozen days are left untouched). Call on launch/resume so a
/// completed day is captured against the planka that applied to it. Ordered BEFORE
/// the weekly recompute in the bootstrap, so on a recompute launch the recent days
/// are frozen at the OLD planka first. No-op until a planka exists.
pub async fn freeze_calories_recent() {
    if local::calorie_goal_amount().await.is_none() {
        return;
    }
    let today = crate::services::local::today_date();
    for i in 1..=14 {
        let _ = day_cached("calories", &fmt(today - Duration::days(i))).await;
    }
}

/// Invalidate cached days affected by a change to `food_id` — every distinct diary
/// date that food appears on (via the diary `food_id` index). A change to a food
/// only ever affects the days it was eaten, so classifying/​editing a food logged
/// today invalidates only today (not a completed day) and the cache stays warm.
pub async fn invalidate_food(food_id: &str) {
    let entries: Vec<api_types::DiaryEntry> =
        crate::services::db::list_by_index("diary", "food_id", food_id).await;
    let dates: HashSet<String> = entries.into_iter().map(|e| e.date).collect();
    for d in dates {
        invalidate_day(&d).await;
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
        "steps" => crate::services::profile::get_steps_planka().unwrap_or(0.0),
        "calories" => local::calorie_goal_amount().await.unwrap_or(0.0),
        "calcium" => CALCIUM_PER_DAY_MG,
        "iron" => iron_per_day_mg(),
        "fiber" => FIBER_PER_DAY_G,
        _ => 0.0,
    }
}

/// Classifier metrics always have data → never Unknown. veg/fruit is derived from
/// tags (a day with none = 0 g). Steps too: a day with no logged steps counts as a
/// MISS (0 < planka), not grey — we're disciplining the user to log every day.
fn is_classifier(key: &str) -> bool {
    key == "veg_fruit" || key == "steps"
}

/// Indicator colour for `key` over the 7 COMPLETED days ending yesterday, read
/// through the per-day cache. Unknown (grey) when the target is unset or a nutrient
/// metric has no data yet.
pub async fn indicator_state(key: &str) -> IndicatorState {
    // Not evaluable yet (e.g. protein before the profile/weight is set).
    if target_for(key).await <= 0.0 {
        return IndicatorState::Unknown;
    }
    let today = crate::services::local::today_date();
    let days: Vec<NaiveDate> = (1..=7).map(|i| today - Duration::days(i)).collect();
    let mut misses = 0u32;
    let mut any_data = false;
    for d in &days {
        let (value, ratio) = day_cached(key, &fmt(*d)).await;
        if value > 0.0 {
            any_data = true;
        }
        // Colour off the FROZEN per-day pair (per-key miss rule: calories = the
        // ±band, the rest = ratio < 1.0), not a fresh compare to the current target.
        if day_missed(key, value, ratio) {
            misses += 1;
        }
    }
    if !is_classifier(key) && !any_data {
        return IndicatorState::Unknown;
    }
    daily_state(misses)
}

/// States for the currently-unlocked indicators, in display order (cached).
pub async fn unlocked_indicator_states() -> Vec<(&'static str, IndicatorState)> {
    let mut out = Vec::new();
    for key in displayed_indicators() {
        out.push((key, indicator_state(key).await));
    }
    out
}

/// Calorie-planka adherence over the last 7 COMPLETED days, as an indicator state.
/// AtMost semantics: a day is a MISS when intake went OVER that day's planka (the
/// opposite of the AtLeast nutrient indicators). Unknown until a planka exists.
/// Days with no logged food are skipped (not counted as adhered). Same bands as
/// [`daily_state`]: 0 over → green · 1–3 → orange · ≥4 → red.
pub async fn calorie_adherence_state() -> IndicatorState {
    if local::calorie_goal_amount().await.is_none() {
        return IndicatorState::Unknown;
    }
    let today = crate::services::local::today_date();
    let mut over = 0u32;
    for i in 1..=7 {
        let d = fmt(today - Duration::days(i));
        let Some(planka) = calorie_planka_on(&d).await else { continue };
        if planka <= 0.0 {
            continue;
        }
        let intake = local::kcal_on(&d).await;
        if intake <= 0.0 {
            continue; // no food logged that day — not an over-eat
        }
        if intake > planka {
            over += 1;
        }
    }
    daily_state(over)
}

/// The full indicator board for the curator food-share. It MUST match the colours
/// the user sees on their own widget, so the DAILY indicators are computed with the
/// exact same `indicator_state` the widget uses — 7 COMPLETED days, TODAY EXCLUDED.
/// (`compute()`'s window includes today, an in-progress day that hasn't met its
/// target yet, which wrongly showed veg-fruit / calcium as orange in the admin
/// while the user's widget — excluding today — showed green.) The three WEEKLY
/// indicators have no per-day `indicator_state`, so they come from `compute()`.
pub async fn share_states() -> Vec<(&'static str, IndicatorState)> {
    let mut out = Vec::new();
    // Calorie-planka adherence (also last 7 COMPLETED days).
    out.push(("calories", calorie_adherence_state().await));
    // Daily indicators — same method + window as the widget.
    for key in ["protein", "veg_fruit", "calcium", "iron", "fiber", "steps"] {
        out.push((key, indicator_state(key).await));
    }
    // Weekly indicators (rolling-7-day sum vs a weekly target) — only `compute()`
    // evaluates these correctly; the widget doesn't surface them.
    for (k, s) in compute().await {
        if matches!(k, "omega3" | "eggs" | "red_meat") {
            out.push((k, s));
        }
    }
    out
}

// ── "Keep them green" gate ───────────────────────────────────────────────────
// The widget nudges the user to keep EVERY unlocked indicator green for a full
// week: 7 GREEN days inside a rolling 8-day window (one day may slip). Counting
// begins the day BEFORE the indicators first appeared (the open date's yesterday,
// the earliest completed day we have on open) and then rolls forward over 8 days.

/// GREEN days required to clear the gate.
pub const GREEN_GATE_DAYS: u32 = 7;
/// Rolling window (in completed days) the required GREEN days must fall within.
const GREEN_GATE_WINDOW: i64 = 8;

/// App-flag holding the date (YYYY-MM-DD) the indicators first became visible.
const GATE_OPEN_KEY: &str = "ind_opened_at";

/// App-flag holding the date the STEP gate (activity week) opened — its rolling
/// window counts from here, so pre-planka days never count. Also the seed for the
/// WEEKLY steps-planka recompute clock (see `letters::maybe_recompute_weekly_steps_planka`),
/// so the first step-up lands one week after the planka was set.
pub(crate) const STEPS_GATE_OPEN_KEY: &str = "steps_gate_opened_at";

/// App-flag holding the date the CALCIUM gate opened — its rolling window counts
/// from here, so days before the calcium goal existed never count.
const CALCIUM_GATE_OPEN_KEY: &str = "calcium_gate_opened_at";

/// The date the indicators "opened" for this user — persisted the first time we
/// evaluate the gate, so the window is anchored to when the nudge began (not to
/// arbitrary earlier diary history).
fn gate_open_date(today: NaiveDate) -> NaiveDate {
    if let Some(s) = crate::services::app_flags::get(GATE_OPEN_KEY) {
        if let Ok(d) = NaiveDate::parse_from_str(&s, "%Y-%m-%d") {
            return d;
        }
    }
    crate::services::app_flags::set(GATE_OPEN_KEY, &fmt(today));
    today
}

/// Did EVERY unlocked indicator meet its target on `date` (per-key green rule:
/// calories = the ±band, the rest = frozen ratio ≥ 1.0)?
async fn all_green_on(date: &str) -> bool {
    for key in UNLOCKED_INDICATORS.iter().copied() {
        let (value, ratio) = day_cached(key, date).await;
        if !day_green(key, value, ratio) {
            return false;
        }
    }
    true
}

/// GREEN days accrued so far toward the gate: the number of completed days in the
/// rolling [`GREEN_GATE_WINDOW`]-day window (yesterday back) on which all unlocked
/// indicators were green — never counting days before the open date's yesterday.
/// Capped at [`GREEN_GATE_DAYS`] (the requirement). `== GREEN_GATE_DAYS` ⇒ cleared.
pub async fn green_gate_progress() -> u32 {
    let today = crate::services::local::today_date();
    let earliest = gate_open_date(today) - Duration::days(1);
    let mut green = 0u32;
    for i in 1..=GREEN_GATE_WINDOW {
        let d = today - Duration::days(i);
        if d < earliest {
            break;
        }
        if all_green_on(&fmt(d)).await {
            green += 1;
        }
    }
    green.min(GREEN_GATE_DAYS)
}

/// GREEN steps-days accrued toward the STEP gate (activity week): completed days
/// from the step-gate open date on which steps met the planka. Steps-only — its
/// own gate, independent of the protein/veg-fruit one. `== GREEN_GATE_DAYS` ⇒ the
/// activity week is cleared (→ next task).
pub async fn steps_gate_progress() -> u32 {
    if !activity_unlocked() {
        return 0;
    }
    let Some(s) = crate::services::app_flags::get(STEPS_GATE_OPEN_KEY) else {
        return 0;
    };
    let Ok(open) = NaiveDate::parse_from_str(&s, "%Y-%m-%d") else {
        return 0;
    };
    let today = crate::services::local::today_date();
    let mut green = 0u32;
    for i in 1..=GREEN_GATE_WINDOW {
        let d = today - Duration::days(i);
        if d < open {
            break; // never count days before the planka was set
        }
        let (_v, ratio) = day_cached("steps", &fmt(d)).await;
        if matches!(ratio, Some(r) if r >= 1.0) {
            green += 1;
        }
    }
    green.min(GREEN_GATE_DAYS)
}

/// GREEN calcium-days accrued toward the CALCIUM gate: completed days from the
/// calcium-gate open date on which calcium met its per-day target (1 g). Calcium
/// isn't cached (`day_cached` computes it live), and stays 0 until foods carry
/// calcium — so the gate only advances once calcium data actually appears.
pub async fn calcium_gate_progress() -> u32 {
    if !calcium_unlocked() {
        return 0;
    }
    let Some(s) = crate::services::app_flags::get(CALCIUM_GATE_OPEN_KEY) else {
        return 0;
    };
    let Ok(open) = NaiveDate::parse_from_str(&s, "%Y-%m-%d") else {
        return 0;
    };
    let today = crate::services::local::today_date();
    let mut green = 0u32;
    for i in 1..=GREEN_GATE_WINDOW {
        let d = today - Duration::days(i);
        if d < open {
            break; // never count days before the calcium goal was set
        }
        let (_v, ratio) = day_cached("calcium", &fmt(d)).await;
        if matches!(ratio, Some(r) if r >= 1.0) {
            green += 1;
        }
    }
    green.min(GREEN_GATE_DAYS)
}

/// One indicator's per-day history for the expanded view's histogram: the 7
/// COMPLETED days (oldest → newest). Each day carries `(date, value, ratio)`, where
/// `ratio` is the FROZEN `value / target` (see [`day_cached`]) — so the bar colours
/// don't shift when the target later changes. `missed` = days with `ratio < 1.0`.
#[derive(Clone)]
pub struct IndicatorSeries {
    pub key: &'static str,
    pub state: IndicatorState,
    pub days: Vec<(String, f64, Option<f64>)>,
    /// Per-day verdict for the bar colours, by THIS indicator's own rule
    /// (calories = the ±50 kcal band, the rest = ratio ≥ 1.0):
    /// Some(true) met · Some(false) missed · None unevaluable (no target).
    pub met_days: Vec<Option<bool>>,
    pub missed: u32,
}

/// Per-day series for every unlocked indicator (cached), for the histograms.
pub async fn unlocked_indicator_series() -> Vec<IndicatorSeries> {
    let today = crate::services::local::today_date();
    // Oldest → newest: today-7 … today-1.
    let dates: Vec<NaiveDate> = (1..=7).rev().map(|i| today - Duration::days(i)).collect();
    let mut out = Vec::new();
    for key in displayed_indicators() {
        let mut days = Vec::with_capacity(dates.len());
        for d in &dates {
            let (value, ratio) = day_cached(key, &fmt(*d)).await;
            days.push((fmt(*d), value, ratio));
        }
        let missed = days
            .iter()
            .filter(|(_, value, ratio)| day_missed(key, *value, *ratio))
            .count() as u32;
        let met_days = days
            .iter()
            .map(|(_, value, ratio)| ratio.map(|_| day_green(key, *value, *ratio)))
            .collect();
        out.push(IndicatorSeries {
            key,
            state: indicator_state(key).await,
            days,
            met_days,
            missed,
        });
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
    let today = fmt(crate::services::local::today_date());
    let mut out = Vec::new();
    for key in displayed_gauges() {
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

    /// The CALORIE indicator band, exactly per the product spec: planka 3000 →
    /// 2951…3049 is green; 2950/2948/3050 already miss. The frozen pair stores
    /// `(value, value/target)`, so the test feeds the same shape.
    #[test]
    fn calorie_band() {
        let pair = |v: f64, planka: f64| (v, Some(v / planka));
        let green = |(v, r): (f64, Option<f64>)| day_green("calories", v, r);
        assert!(green(pair(2951.0, 3000.0)));
        assert!(green(pair(3000.0, 3000.0)));
        assert!(green(pair(3049.0, 3000.0)));
        assert!(!green(pair(2950.0, 3000.0)));
        assert!(!green(pair(3050.0, 3000.0)));
        assert!(!green(pair(2948.0, 3000.0)));
        // No data / no target → never green, and a miss only when judgeable.
        assert!(!day_green("calories", 0.0, Some(0.0)));
        assert!(!day_green("calories", 3000.0, None));
        assert!(!day_missed("calories", 3000.0, None));
        assert!(day_missed("calories", 2900.0, Some(2900.0 / 3000.0)));
        // Other indicators keep the AtLeast rule.
        assert!(day_green("protein", 100.0, Some(1.0)));
        assert!(!day_green("protein", 90.0, Some(0.9)));
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

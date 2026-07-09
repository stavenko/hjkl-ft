//! Weekly recommendation engine — a PURE, total, deterministic module (no I/O,
//! no Leptos, no IndexedDB), unit-testable exactly like `weight_trend.rs`.
//!
//! The §8 backend/frontend separation lives INSIDE this module: [`decide`] is the
//! "backend" (all logic) and the returned [`Card`] is the contract the dumb
//! component renders. The component renders strictly by `state` + `levers`; it
//! never re-derives a decision.
//!
//! Invariants honoured BY CONSTRUCTION (§7):
//! - §7.1 In HARD the calorie lever is `false` and `new_cal` is `None` — the
//!   component never even constructs a calorie button.
//! - §7.2 `new_cal`, when `Some`, is never below the per-sex calorie floor — the
//!   only write path clamps before returning.
//! - §7.3 The HARD predicate is state-machine row #1, evaluated BEFORE the rows
//!   that run the calorie-cut lever; in HARD `decide` returns immediately, so the
//!   control law is never executed.
//! - §7.4 `birth_year == None` is the FIRST branch and returns `NeedBirthYear`;
//!   no code path ever substitutes a default age.

use api_types::{StepEntry, WeightEntry};
use serde::{Deserialize, Serialize};

use crate::services::profile::{CourseGoal, Sex};
use crate::services::weight_trend::{self, BalanceState, WeightTrend};

// ── §9 approved constants ────────────────────────────────────────────────────

/// Activity multiplier applied to RMR, by median daily steps (the §-table).
const CAL_FLOOR_MALE: f64 = 1500.0;
const CAL_FLOOR_FEMALE: f64 = 1200.0;
const TISSUE_KCAL_PER_KG: f64 = 7700.0;
const STEPS_INCREMENT: u32 = 1000;
const STEPS_CEILING: u32 = 15000;
/// Lookback window for the trend/median (uses whatever days exist within it).
const WINDOW_DAYS: i64 = 14;
/// First recommendation after a WEEK of complete logging — the user reaches the
/// card (ch3) only after ~a week of all three diaries. We do NOT wait for 14 days
/// or for a "confident" direction: "not confidently up and not confidently down"
/// is read as maintenance (a plateau), early, and corrected weekly.
const MIN_DATA_DAYS: usize = 7;
const RATIO_HARD: f64 = 0.85;
const RATIO_CLEAN: f64 = 0.95;

// ── §8 contract types ─────────────────────────────────────────────────────────

/// `NeedBirthYear` is the §7.4 surfaced state, beyond §8's six.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CardState {
    Hard,
    OnTrack,
    Plateau,
    Surplus,
    Soft,
    InsufficientData,
    NeedBirthYear,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Levers {
    pub calories: bool,
    pub steps: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Computed {
    pub new_cal: Option<f64>,
    pub new_steps: Option<u32>,
}

/// Which way the weight is going, for direction-aware copy (Soft #2/#3, and the
/// maintenance-goal messages). Falling/Rising = confidently down/up; Flat = "not
/// confidently up and not confidently down" (= maintenance).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TrendDir {
    Falling,
    Flat,
    Rising,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Card {
    pub state: CardState,
    pub levers: Levers,
    pub computed: Computed,
    /// Echoes `avg_intake_14d` for the card.
    pub current_intake_avg: Option<f64>,
    /// The course goal in effect (so the dumb card picks the right copy).
    #[serde(default = "default_goal")]
    pub goal: CourseGoalDto,
    /// Weight direction, for direction-aware copy.
    #[serde(default = "default_dir")]
    pub trend: TrendDir,
}

/// Serde-friendly mirror of `profile::CourseGoal` (kept here so the contract type
/// owns its serialization).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CourseGoalDto {
    Lose,
    Maintain,
}
impl From<CourseGoal> for CourseGoalDto {
    fn from(g: CourseGoal) -> Self {
        match g {
            CourseGoal::Lose => CourseGoalDto::Lose,
            CourseGoal::Maintain => CourseGoalDto::Maintain,
            // TODO: the weekly card doesn't model a surplus yet; treat "gain" as
            // maintenance until surplus planning is added.
            CourseGoal::Gain => CourseGoalDto::Maintain,
        }
    }
}
fn default_goal() -> CourseGoalDto {
    CourseGoalDto::Lose
}
fn default_dir() -> TrendDir {
    TrendDir::Flat
}

impl Card {
    fn empty(state: CardState, current_intake_avg: Option<f64>, goal: CourseGoalDto) -> Self {
        Card {
            state,
            levers: Levers { calories: false, steps: false },
            computed: Computed { new_cal: None, new_steps: None },
            current_intake_avg,
            goal,
            trend: TrendDir::Flat,
        }
    }
}

// ── Engine input ──────────────────────────────────────────────────────────────

/// All inputs the caller gathers from existing services and hands to [`decide`]
/// so the engine stays pure.
pub struct EngineInput {
    pub sex: Option<Sex>,
    pub height_cm: Option<f64>,
    pub birth_year: Option<i32>,
    /// chrono year, injected for purity/testability.
    pub today_year: i32,
    /// raw 14d+; the trend is computed inside.
    pub weight_entries: Vec<WeightEntry>,
    /// local::avg_daily_kcal(14) — the logged-days-only mean.
    pub avg_intake_14d: Option<f64>,
    /// count of days with diary in the last 14 (coverage numerator).
    pub logged_days_14d: u32,
    /// raw; the 14d median is computed inside.
    pub step_entries: Vec<StepEntry>,
    /// goals: nutrient == "Calories" && AtMost && amount > 0.
    pub current_cal_planka: Option<f64>,
    /// The current daily Steps goal (or the default planka).
    pub current_steps_target: u32,
    /// The course goal (Lose | Maintain). Maintenance suppresses the calorie lever.
    pub goal: CourseGoal,
}

// ── Pure helpers ───────────────────────────────────────────────────────────────

/// Mifflin-St Jeor resting metabolic rate.
pub(crate) fn mifflin_rmr(sex: Sex, weight_kg: f64, height_cm: f64, age: i32) -> f64 {
    let s = match sex {
        Sex::Male => 5.0,
        Sex::Female => -161.0,
    };
    10.0 * weight_kg + 6.25 * height_cm - 5.0 * age as f64 + s
}

/// Activity factor from median daily steps (the §-table).
///
/// Edge semantics (documented): each named upper bound is INCLUSIVE of the lower
/// tier — `<5000`, `5000..=8000`, `8001..=12000`, `12001..=15000`, `>15000`.
pub(crate) fn activity_factor(median_steps: f64) -> f64 {
    if median_steps < 5000.0 {
        1.30
    } else if median_steps <= 8000.0 {
        1.45
    } else if median_steps <= 12000.0 {
        1.60
    } else if median_steps <= 15000.0 {
        1.75
    } else {
        1.90
    }
}

/// Median daily steps over the trailing `WINDOW_DAYS`, anchored at the latest
/// step entry (matching `weight_trend`'s latest-anchored window). Multiple
/// entries on the same calendar day are summed into that day's total. No steps
/// in the window → 0.0.
pub(crate) fn median_steps(entries: &[StepEntry]) -> f64 {
    use std::collections::BTreeMap;
    let mut by_day: BTreeMap<chrono::NaiveDate, u64> = BTreeMap::new();
    let mut latest: Option<chrono::NaiveDate> = None;
    for e in entries {
        let Ok(d) = chrono::NaiveDate::parse_from_str(&e.date, "%Y-%m-%d") else {
            continue;
        };
        latest = Some(latest.map_or(d, |l: chrono::NaiveDate| l.max(d)));
        *by_day.entry(d).or_insert(0) += e.steps as u64;
    }
    let Some(latest) = latest else {
        return 0.0;
    };
    let window_start = latest - chrono::Duration::days(WINDOW_DAYS - 1);
    let mut vals: Vec<f64> = by_day
        .into_iter()
        .filter(|(d, _)| *d >= window_start)
        .map(|(_, s)| s as f64)
        .collect();
    if vals.is_empty() {
        return 0.0;
    }
    vals.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let n = vals.len();
    if n % 2 == 1 {
        vals[n / 2]
    } else {
        (vals[n / 2 - 1] + vals[n / 2]) / 2.0
    }
}

fn round_to_10(x: f64) -> f64 {
    (x / 10.0).round() * 10.0
}

fn cal_floor(sex: Sex) -> f64 {
    match sex {
        Sex::Male => CAL_FLOOR_MALE,
        Sex::Female => CAL_FLOOR_FEMALE,
    }
}

/// Distinct measurement days the trend saw (any variant).
fn trend_days(t: &WeightTrend) -> usize {
    match t {
        WeightTrend::Insufficient { days } => *days,
        WeightTrend::Tentative { days, .. } => *days,
        WeightTrend::Estimated { days, .. } => *days,
    }
}

/// Distinct step-logging days within the trailing `WINDOW_DAYS` (anchored latest).
fn steps_days(entries: &[StepEntry]) -> usize {
    use std::collections::BTreeSet;
    let days: BTreeSet<chrono::NaiveDate> = entries
        .iter()
        .filter_map(|e| chrono::NaiveDate::parse_from_str(&e.date, "%Y-%m-%d").ok())
        .collect();
    let Some(latest) = days.iter().max().copied() else { return 0 };
    let start = latest - chrono::Duration::days(WINDOW_DAYS - 1);
    days.iter().filter(|d| **d >= start).count()
}

/// The data gate (§4, per the user's "a week of all three diaries"): at least a
/// week of WEIGHT, DIARY and STEPS days each, plus an intake mean. We do NOT gate
/// on a confident direction — flat/uncertain just reads as maintenance downstream.
fn gate_open(input: &EngineInput, trend: &WeightTrend) -> bool {
    trend_days(trend) >= MIN_DATA_DAYS
        && (input.logged_days_14d as usize) >= MIN_DATA_DAYS
        && steps_days(&input.step_entries) >= MIN_DATA_DAYS
        && input.avg_intake_14d.is_some()
}

/// Per-day slope (kg/day) from the trend, or `None` for Insufficient.
fn slope_kg_per_day(trend: &WeightTrend) -> Option<f64> {
    match trend {
        WeightTrend::Estimated { slope_kg_per_week, .. }
        | WeightTrend::Tentative { slope_kg_per_week, .. } => Some(slope_kg_per_week / 7.0),
        WeightTrend::Insufficient { .. } => None,
    }
}

/// Direction from the trend's balance: confident down = Falling, confident up =
/// Rising, everything else (not confidently up and not confidently down) = Flat
/// (= maintenance). This is the user's rule: "не вниз и не вверх → поддержка".
fn trend_dir(balance: BalanceState) -> TrendDir {
    match balance {
        BalanceState::Deficit => TrendDir::Falling,
        BalanceState::Surplus => TrendDir::Rising,
        BalanceState::Maintenance => TrendDir::Flat,
    }
}

/// The CALORIES lever (§6). Returns `(levers.calories, new_cal)` after the
/// `cal_floor` clamp. §7.2: any returned `Some` is `>= cal_floor`.
fn calorie_lever(current_cal_planka: Option<f64>, sex: Sex) -> (bool, Option<f64>) {
    let floor = cal_floor(sex);
    let Some(planka) = current_cal_planka else {
        // No planka set yet → can't compute a cut; steps still offered.
        return (false, None);
    };
    let mut new_cal = round_to_10(planka * 0.95);
    if new_cal < floor {
        new_cal = floor;
        if floor >= planka {
            // Can't cut further → HIDE the calorie button.
            return (false, None);
        }
    }
    (true, Some(new_cal))
}

/// The STEPS lever (§6): `min(current_steps_target + 1000, 15000)`.
fn steps_lever(current_steps_target: u32) -> u32 {
    (current_steps_target + STEPS_INCREMENT).min(STEPS_CEILING)
}

// ── decide ─────────────────────────────────────────────────────────────────────

/// The whole §3–§6 decision, as one linear pure function.
pub fn decide(input: &EngineInput) -> Card {
    let goal: CourseGoalDto = input.goal.into();

    // (1) §7.4 AGE-MISSING GATE — FIRST. No predicate, no default age.
    let Some(birth_year) = input.birth_year else {
        return Card::empty(CardState::NeedBirthYear, input.avg_intake_14d, goal);
    };
    let age = input.today_year - birth_year;

    // (2) RMR inputs: sex + height + latest logged weight.
    let trend = weight_trend::weight_trend(&input.weight_entries, WINDOW_DAYS);
    let latest_weight = input.weight_entries.last().map(|e| e.weight_kg);
    let (Some(sex), Some(height_cm), Some(weight_kg)) = (input.sex, input.height_cm, latest_weight)
    else {
        return Card::empty(CardState::InsufficientData, input.avg_intake_14d, goal);
    };
    let rmr = mifflin_rmr(sex, weight_kg, height_cm, age);

    // (3) activity factor by median steps; (4) TDEE floor.
    let med_steps = median_steps(&input.step_entries);
    let tdee_floor = rmr * activity_factor(med_steps);

    // (8) data gate — a week of all three diaries; failure → InsufficientData (#0).
    if !gate_open(input, &trend) {
        return Card::empty(CardState::InsufficientData, input.avg_intake_14d, goal);
    }

    let avg_intake = input.avg_intake_14d.expect("gate requires avg_intake_14d");
    // Past the gate the trend has >= 7 days, so a slope exists.
    let slope = slope_kg_per_day(&trend).expect("gate requires >= 7 weight days");

    // (6) computed TDEE; (7) ratio.
    let computed_tdee = avg_intake - slope * TISSUE_KCAL_PER_KG;
    let ratio = computed_tdee / tdee_floor;

    let balance = trend.balance();
    let dir = trend_dir(balance);

    // (9) STATE MACHINE — first match wins.

    // #1 HARD: data-quality veto, goal-independent, BEFORE any lever (§7.3). Calorie
    // lever NEVER entered here.
    if computed_tdee < rmr || ratio < RATIO_HARD {
        let mut c = Card::empty(CardState::Hard, input.avg_intake_14d, goal);
        c.trend = dir;
        return c;
    }

    // #2 OnTrack: clean ratio AND confidently falling toward target.
    let state = if ratio >= RATIO_CLEAN {
        match dir {
            TrendDir::Falling => CardState::OnTrack,
            TrendDir::Rising => CardState::Surplus,
            TrendDir::Flat => CardState::Plateau, // reachable: "не вниз и не вверх"
        }
    } else {
        CardState::Soft // RATIO_HARD <= ratio < RATIO_CLEAN
    };

    // Levers (§6). OnTrack carries none.
    let (cal_on, new_cal, steps_on, new_steps) = match state {
        CardState::OnTrack => (false, None, false, None),
        _ => {
            let (c_on, nc) = calorie_lever(input.current_cal_planka, sex);
            (c_on, nc, true, Some(steps_lever(input.current_steps_target)))
        }
    };

    let mut card = Card {
        state,
        levers: Levers { calories: cal_on, steps: steps_on },
        computed: Computed { new_cal, new_steps },
        current_intake_avg: input.avg_intake_14d,
        goal,
        trend: dir,
    };

    // MAINTENANCE GOAL: never suggest lowering the calorie planka. Steps are only
    // offered when drifting UP (adding activity ≠ lowering the planka). Flat is the
    // success state; falling is just informational. The dumb card picks the copy by
    // (goal, trend); here we only enforce the levers.
    if goal == CourseGoalDto::Maintain {
        card.levers.calories = false;
        card.computed.new_cal = None;
        card.levers.steps = dir == TrendDir::Rising;
        if dir != TrendDir::Rising {
            card.computed.new_steps = None;
        }
    }

    card
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(date: &str, kg: f64) -> WeightEntry {
        WeightEntry {
            id: date.to_string(),
            date: date.to_string(),
            weight_kg: kg,
            no_water: false,
            no_food: false,
            no_wash: false,
            used_toilet: false,
            morning: false,
            created_at: String::new(),
            updated_at: String::new(),
        }
    }

    fn step(date: &str, steps: u32) -> StepEntry {
        StepEntry {
            id: date.to_string(),
            date: date.to_string(),
            steps,
            created_at: String::new(),
            updated_at: String::new(),
        }
    }

    /// 14 daily weigh-ins (June 1..14), drifting `slope_per_day` kg/day from `base`,
    /// with tiny alternating noise so the OLS confidence is high but not perfect.
    fn weights(base: f64, slope_per_day: f64) -> Vec<WeightEntry> {
        (1..=14u32)
            .map(|dd| {
                let noise = if dd % 2 == 0 { 0.02 } else { -0.02 };
                entry(&format!("2026-06-{dd:02}"), base + slope_per_day * (dd as f64 - 1.0) + noise)
            })
            .collect()
    }

    /// 14 days of identical steps (so the median is exactly `s`).
    fn steps_flat(s: u32) -> Vec<StepEntry> {
        (1..=14u32).map(|dd| step(&format!("2026-06-{dd:02}"), s)).collect()
    }

    /// A baseline input: male, 180 cm, born 1990, with 14d of clean falling
    /// weight, full coverage, low-ish steps. Callers tweak fields.
    fn base_input() -> EngineInput {
        EngineInput {
            sex: Some(Sex::Male),
            height_cm: Some(180.0),
            birth_year: Some(1990),
            today_year: 2026,
            weight_entries: weights(90.0, -0.1), // ~0.7 kg/week down, confident
            avg_intake_14d: Some(2000.0),
            logged_days_14d: 14,
            step_entries: steps_flat(4000), // tier 1.30
            current_cal_planka: Some(2000.0),
            current_steps_target: 7000,
            goal: CourseGoal::Lose,
        }
    }

    /// 14 days of flat weight (slope ~0, low confidence → Maintenance/Flat).
    fn weights_flat(base: f64) -> Vec<WeightEntry> {
        weights(base, 0.0)
    }

    // ── §7.4 age-missing ─────────────────────────────────────────────────────

    #[test]
    fn need_birth_year_when_unset_regardless_of_other_inputs() {
        let mut i = base_input();
        i.birth_year = None;
        let c = decide(&i);
        assert_eq!(c.state, CardState::NeedBirthYear);
        assert!(!c.levers.calories && !c.levers.steps);
        assert_eq!(c.computed.new_cal, None);
        assert_eq!(c.computed.new_steps, None);
        assert_eq!(c.current_intake_avg, Some(2000.0));
    }

    #[test]
    fn no_silent_default_age_short_circuits_before_rmr() {
        // Even with a current planka and rich data, missing age must NOT compute.
        let mut i = base_input();
        i.birth_year = None;
        i.current_cal_planka = Some(2500.0);
        assert_eq!(decide(&i).state, CardState::NeedBirthYear);
    }

    // ── InsufficientData gates ─────────────────────────────────────────────────

    #[test]
    fn insufficient_when_weight_days_below_week() {
        let mut i = base_input();
        // Only 5 distinct weight days → < 7-day data gate.
        i.weight_entries = (10..=14u32)
            .map(|dd| entry(&format!("2026-06-{dd:02}"), 90.0 - 0.1 * dd as f64))
            .collect();
        assert_eq!(decide(&i).state, CardState::InsufficientData);
    }

    #[test]
    fn insufficient_when_diary_days_below_week() {
        let mut i = base_input();
        i.logged_days_14d = 5; // < 7
        assert_eq!(decide(&i).state, CardState::InsufficientData);
    }

    #[test]
    fn insufficient_when_steps_days_below_week() {
        let mut i = base_input();
        // Only 5 step days in the window → < 7.
        i.step_entries = (10..=14u32).map(|dd| step(&format!("2026-06-{dd:02}"), 4000)).collect();
        assert_eq!(decide(&i).state, CardState::InsufficientData);
    }

    #[test]
    fn enough_when_exactly_a_week_of_each() {
        // 7 weight + 7 diary + 7 step days passes the gate (no 14-day wait).
        let mut i = base_input();
        i.weight_entries = (8..=14u32)
            .map(|dd| entry(&format!("2026-06-{dd:02}"), 90.0 - 0.1 * (dd as f64 - 8.0)))
            .collect();
        i.logged_days_14d = 7;
        i.step_entries = (8..=14u32).map(|dd| step(&format!("2026-06-{dd:02}"), 4000)).collect();
        i.avg_intake_14d = Some(2400.0);
        assert_ne!(decide(&i).state, CardState::InsufficientData);
    }

    #[test]
    fn insufficient_when_no_intake_mean() {
        let mut i = base_input();
        i.avg_intake_14d = None;
        let c = decide(&i);
        assert_eq!(c.state, CardState::InsufficientData);
        assert_eq!(c.current_intake_avg, None);
    }

    #[test]
    fn insufficient_when_missing_sex_height_or_weight() {
        let mut i = base_input();
        i.sex = None;
        assert_eq!(decide(&i).state, CardState::InsufficientData);
        let mut i = base_input();
        i.height_cm = None;
        assert_eq!(decide(&i).state, CardState::InsufficientData);
        let mut i = base_input();
        i.weight_entries = Vec::new();
        assert_eq!(decide(&i).state, CardState::InsufficientData);
    }

    // ── HARD ───────────────────────────────────────────────────────────────────

    #[test]
    fn hard_via_computed_tdee_below_rmr() {
        // Flat weight (slope ~0 → computed_TDEE ~= intake) with a very low intake,
        // so computed_TDEE < RMR. Use confident flat? Flat isn't confident, so make
        // a confident slight descent but tiny, and a tiny intake.
        let mut i = base_input();
        i.weight_entries = weights(90.0, -0.1); // confident down
        i.avg_intake_14d = Some(800.0); // far below RMR
        let rmr = mifflin_rmr(Sex::Male, 90.0, 180.0, 36);
        // computed_TDEE = 800 - (-0.1)*7700 = 1570; RMR(90kg,180,36) ~ 1690 → < RMR.
        assert!(rmr > 1570.0, "sanity: rmr {rmr}");
        let c = decide(&i);
        assert_eq!(c.state, CardState::Hard);
        assert!(!c.levers.calories);
        assert_eq!(c.computed.new_cal, None);
    }

    #[test]
    fn hard_via_ratio_below_085() {
        // computed_TDEE >= RMR but ratio < 0.85: make the activity factor high
        // (lots of steps) so TDEE_floor is large relative to computed_TDEE.
        let mut i = base_input();
        i.step_entries = steps_flat(16000); // factor 1.90
        i.avg_intake_14d = Some(1900.0);
        i.weight_entries = weights(90.0, -0.05); // mild confident descent
        let c = decide(&i);
        assert_eq!(c.state, CardState::Hard);
        assert!(!c.levers.calories);
        assert_eq!(c.computed.new_cal, None);
    }

    #[test]
    fn hard_blocks_calorie_control_law_even_with_planka() {
        let mut i = base_input();
        i.avg_intake_14d = Some(800.0); // force computed_TDEE < RMR
        i.current_cal_planka = Some(2500.0); // a planka IS set
        let c = decide(&i);
        assert_eq!(c.state, CardState::Hard);
        assert!(!c.levers.calories, "calorie lever must be off in HARD");
        assert_eq!(c.computed.new_cal, None, "new_cal must not be computed in HARD");
    }

    // ── OnTrack / Plateau / Surplus / Soft ─────────────────────────────────────

    #[test]
    fn on_track_when_clean_and_confident_down() {
        // High intake so computed_TDEE comfortably above floor (ratio >= 0.95) and
        // confident descent.
        let mut i = base_input();
        i.step_entries = steps_flat(4000); // factor 1.30
        i.weight_entries = weights(90.0, -0.1); // confident down
        i.avg_intake_14d = Some(2400.0);
        let c = decide(&i);
        assert_eq!(c.state, CardState::OnTrack);
        assert!(!c.levers.calories && !c.levers.steps);
        assert_eq!(c.computed.new_cal, None);
    }

    #[test]
    fn plateau_reachable_when_flat() {
        // The user's rule: "не вниз и не вверх → поддержка (плато)". Flat weight is
        // Maintenance, which now reaches Plateau (not InsufficientData). Clean ratio.
        let mut i = base_input();
        i.weight_entries = weights_flat(80.0); // slope ~0 → Maintenance/Flat
        i.step_entries = steps_flat(4000); // factor 1.30 → floor ~2275
        i.avg_intake_14d = Some(2300.0); // ratio ~1.01 (>= 0.95), flat
        let c = decide(&i);
        assert_eq!(c.state, CardState::Plateau);
        assert_eq!(c.trend, TrendDir::Flat);
        assert!(c.levers.calories); // lose goal → offer the cut
        assert!(c.levers.steps);
    }

    #[test]
    fn surplus_when_clean_ratio_and_confident_up() {
        // Confident rising weight (+0.05 kg/day) with a high intake so the ratio
        // stays clean (>= 0.95). factor 1.30 (steps 4000).
        let mut i = base_input();
        i.weight_entries = weights(70.0, 0.05); // confident up
        i.avg_intake_14d = Some(2600.0); // high → clean ratio (~1.03)
        let c = decide(&i);
        assert_eq!(c.state, CardState::Surplus);
        assert!(c.levers.calories); // planka 2000 → cut to 1900 (>= floor 1500)
        assert!(c.levers.steps);
        assert_eq!(c.computed.new_cal, Some(1900.0));
        assert_eq!(c.computed.new_steps, Some(8000));
    }

    #[test]
    fn soft_when_ratio_between_085_and_095() {
        // factor 1.30 (steps 4000), confident descent -0.05 kg/day, latest ~89.35 kg
        // → RMR ~1843.5, floor ~2396.6. computed_TDEE = intake + 0.05*7700.
        // intake 1750 → ct 2135 → ratio ~0.891 (in [0.85, 0.95)) → Soft.
        let mut i = base_input();
        i.step_entries = steps_flat(4000); // factor 1.30
        i.weight_entries = weights(90.0, -0.05); // mild confident descent
        i.avg_intake_14d = Some(1750.0);
        let c = decide(&i);
        assert_eq!(c.state, CardState::Soft);
        assert!(c.levers.calories);
        assert!(c.levers.steps);
    }

    // ── cal_floor clamp ──────────────────────────────────────────────────────────

    #[test]
    fn cal_floor_clamps_but_keeps_lever_when_can_still_cut() {
        // planka just above floor so 0.95*planka < floor but floor < planka:
        // new_cal == floor and lever stays true.
        let (on, new_cal) = calorie_lever(Some(1550.0), Sex::Male); // 0.95*1550=1472.5→1470 < 1500
        assert!(on);
        assert_eq!(new_cal, Some(1500.0));
    }

    #[test]
    fn cal_floor_hides_lever_when_cant_cut() {
        // planka <= floor (floor >= planka) → can't cut further → lever off, None.
        let (on, new_cal) = calorie_lever(Some(1500.0), Sex::Male);
        assert!(!on);
        assert_eq!(new_cal, None);
        let (on, new_cal) = calorie_lever(Some(1400.0), Sex::Male);
        assert!(!on);
        assert_eq!(new_cal, None);
    }

    #[test]
    fn cal_floor_differs_by_sex() {
        // Same planka 1300: for a man (floor 1500) can't cut → off; for a woman
        // (floor 1200) 0.95*1300=1235 >= 1200 → on, value 1240 (round_to_10).
        let (on_m, cal_m) = calorie_lever(Some(1300.0), Sex::Male);
        assert!(!on_m);
        assert_eq!(cal_m, None);
        let (on_f, cal_f) = calorie_lever(Some(1300.0), Sex::Female);
        assert!(on_f);
        assert_eq!(cal_f, Some(1240.0));
    }

    #[test]
    fn no_planka_hides_calorie_lever_but_steps_still_offered() {
        // current_cal_planka None → calorie lever off; in a Surplus state steps stay.
        let mut i = base_input();
        i.weight_entries = weights(70.0, 0.05); // confident up → Surplus
        i.avg_intake_14d = Some(2600.0);
        i.current_cal_planka = None;
        let c = decide(&i);
        assert_eq!(c.state, CardState::Surplus);
        assert!(!c.levers.calories);
        assert_eq!(c.computed.new_cal, None);
        assert!(c.levers.steps);
        assert_eq!(c.computed.new_steps, Some(8000));
    }

    #[test]
    fn returned_new_cal_is_never_below_floor() {
        // Property over a sweep of plankas for both sexes.
        for &sex in &[Sex::Male, Sex::Female] {
            let floor = cal_floor(sex);
            for planka in (1000..4000).step_by(37) {
                let (_, new_cal) = calorie_lever(Some(planka as f64), sex);
                if let Some(v) = new_cal {
                    assert!(v >= floor, "new_cal {v} < floor {floor} for planka {planka}");
                }
            }
        }
    }

    // ── steps lever ──────────────────────────────────────────────────────────────

    #[test]
    fn steps_lever_increment_and_ceiling() {
        assert_eq!(steps_lever(7000), 8000);
        assert_eq!(steps_lever(14500), 15000); // 15500 clamped to ceiling
        assert_eq!(steps_lever(15000), 15000);
    }

    // ── activity_factor tiers ──────────────────────────────────────────────────

    #[test]
    fn activity_factor_tier_boundaries() {
        assert_eq!(activity_factor(4999.0), 1.30);
        assert_eq!(activity_factor(5000.0), 1.45);
        assert_eq!(activity_factor(8000.0), 1.45);
        assert_eq!(activity_factor(8001.0), 1.60);
        assert_eq!(activity_factor(12000.0), 1.60);
        assert_eq!(activity_factor(12001.0), 1.75);
        assert_eq!(activity_factor(15000.0), 1.75);
        assert_eq!(activity_factor(15001.0), 1.90);
    }

    #[test]
    fn median_steps_empty_window_is_zero() {
        assert_eq!(median_steps(&[]), 0.0);
        assert_eq!(activity_factor(median_steps(&[])), 1.30);
    }

    #[test]
    fn median_steps_basic() {
        // Odd count.
        let e = vec![step("2026-06-12", 3000), step("2026-06-13", 9000), step("2026-06-14", 6000)];
        assert_eq!(median_steps(&e), 6000.0);
        // Even count → average of the two middles.
        let e = vec![
            step("2026-06-11", 2000),
            step("2026-06-12", 4000),
            step("2026-06-13", 6000),
            step("2026-06-14", 8000),
        ];
        assert_eq!(median_steps(&e), 5000.0);
    }

    #[test]
    fn median_steps_excludes_out_of_window() {
        // An old huge day 30 days before the latest is dropped.
        let mut e = vec![step("2026-05-01", 30000)];
        for dd in 10..=14u32 {
            e.push(step(&format!("2026-06-{dd:02}"), 5000));
        }
        assert_eq!(median_steps(&e), 5000.0);
    }

    // ── RMR exactness ────────────────────────────────────────────────────────────

    // ── Maintenance goal: never suggest lowering calories ──────────────────────

    #[test]
    fn maintain_flat_is_success_no_levers() {
        let mut i = base_input();
        i.goal = CourseGoal::Maintain;
        i.weight_entries = weights_flat(80.0);
        i.step_entries = steps_flat(4000);
        i.avg_intake_14d = Some(2300.0); // flat, clean
        let c = decide(&i);
        assert_eq!(c.trend, TrendDir::Flat);
        assert!(!c.levers.calories, "maintenance never lowers the calorie planka");
        assert!(!c.levers.steps, "holding steady → no nudge");
        assert_eq!(c.computed.new_cal, None);
    }

    #[test]
    fn maintain_rising_offers_steps_not_calorie_cut() {
        let mut i = base_input();
        i.goal = CourseGoal::Maintain;
        i.weight_entries = weights(70.0, 0.05); // confident up
        i.avg_intake_14d = Some(2600.0);
        i.current_cal_planka = Some(2000.0);
        let c = decide(&i);
        assert_eq!(c.trend, TrendDir::Rising);
        assert!(!c.levers.calories, "maintenance never lowers the calorie planka");
        assert_eq!(c.computed.new_cal, None);
        assert!(c.levers.steps, "drifting up → add activity");
        assert_eq!(c.computed.new_steps, Some(8000));
    }

    #[test]
    fn maintain_never_cuts_even_in_soft_or_plateau() {
        // Whatever the lose-machine would do, maintenance keeps the calorie lever off.
        for w in [weights(90.0, -0.05), weights_flat(80.0), weights(70.0, 0.05)] {
            let mut i = base_input();
            i.goal = CourseGoal::Maintain;
            i.weight_entries = w;
            i.avg_intake_14d = Some(1750.0);
            let c = decide(&i);
            if c.state != CardState::Hard && c.state != CardState::InsufficientData {
                assert!(!c.levers.calories, "maintenance must never offer a calorie cut");
                assert_eq!(c.computed.new_cal, None);
            }
        }
    }

    #[test]
    fn mifflin_rmr_exact_values() {
        // Male: 10*80 + 6.25*180 - 5*30 + 5 = 800 + 1125 - 150 + 5 = 1780.
        assert!((mifflin_rmr(Sex::Male, 80.0, 180.0, 30) - 1780.0).abs() < 1e-9);
        // Female: 10*60 + 6.25*165 - 5*30 - 161 = 600 + 1031.25 - 150 - 161 = 1320.25.
        assert!((mifflin_rmr(Sex::Female, 60.0, 165.0, 30) - 1320.25).abs() < 1e-9);
    }
}

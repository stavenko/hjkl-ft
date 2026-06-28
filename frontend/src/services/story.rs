use std::cell::RefCell;
use std::collections::HashSet;

use leptos::*;
use serde::{Deserialize, Serialize};

use crate::services::story_dsl::{self, Engine};
use crate::services::{db, local, profile, subscription, summary, sync};

/// Task flag: the user committed to wanting a new body (chapter 1, intro).
pub const WANT_NEW_BODY: &str = "want_new_body";
/// Task flag: the user confirmed the language is set up the way they want.
pub const LANGUAGE_CONFIGURED: &str = "language_configured";
/// Task flag: the user received and tapped the test push notification.
pub const NOTIFICATION_RECEIVED: &str = "notification_received";
/// Milestone (event): the user enabled the weigh-in reminder at least once.
/// Recorded on the enable action — NOT mirrored from the current schedule state.
pub const WEIGH_IN_REMINDER: &str = "weigh_in_reminder_enabled";
/// Milestone (event): the user made a weight measurement. Recorded on the save
/// action — NOT derived by reading the weight entries each time.
pub const FIRST_WEIGH: &str = "first_measurement_done";
/// Milestone: the meal/steps reminders have been unlocked (set by the "first
/// food entries" section, where the user is taught to enable them).
pub const MEAL_REMINDERS_UNLOCKED: &str = "meal_reminders_unlocked";
/// Trigger armed when the "first food entries" section is opened. The task is
/// completed only by a food entry made AFTER that — earlier entries don't count.
pub const FIRST_FOOD_ARMED: &str = "first_food_armed";
/// Milestone (event): a food was entered into the diary while the trigger was
/// armed — i.e. the "first food entries" task is done.
pub const FIRST_FOOD_DONE: &str = "first_food_done";
/// Milestone (event): the user repeated a past day's food into today via the
/// diary's "Repeat today" action — the "why keep a diary" section task.
pub const FOOD_REPEATED: &str = "food_repeated";
/// Milestone (event): the user enabled the steps reminder at least once.
pub const STEPS_REMINDER: &str = "steps_reminder_enabled";
/// Milestone (event): the user recorded their steps at least once.
pub const FIRST_STEPS: &str = "first_steps_done";
/// Milestone (event): the user created (finalized) a cooked dish (recipe).
pub const COOKING_DISH_CREATED: &str = "cooking_dish_created";
/// Milestone (event): a cooked dish (recipe) was added to the diary.
pub const COOKING_DISH_IN_DIARY: &str = "cooking_dish_in_diary";
/// Milestone (event): the user recorded inedible waste (bones/pits) on a diary entry.
pub const BONES_WASTE_ENTERED: &str = "bones_waste_entered";
pub const RESTAURANT_FOOD_ENTERED: &str = "restaurant_food_entered";
pub const PROGRESS_PHOTOS_TAKEN: &str = "progress_photos_taken";
/// Set once the user picks their sex in settings (setup section task).
pub const SEX_SELECTED: &str = "sex_selected";
/// Set once the user saves a valid year of birth in settings (accounting section
/// task `age`). A STORY flag — distinct from the synced profile.birth_year value.
pub const BIRTH_YEAR_SET: &str = "birth_year_set";
/// Chapter 2 / s6: set when the meal-split section is opened. While set, the
/// diary page groups the day's entries by derived meal instead of a flat list.
pub const MEAL_SPLIT_UNLOCKED: &str = "meal_split_unlocked";
/// Chapter 2 / s7: set when the "eating at night" section is opened and the
/// user views today's evening feedback. Completes that section's task.
pub const NIGHT_FEEDBACK_VIEWED: &str = "night_feedback_viewed";
/// Chapter 3: the current daily steps planka (the "Почему не уходит вес" task —
/// hit it 7 days in a row). A day counts toward the `steps_planka` source when
/// its logged steps reach this value.
pub const STEPS_PLANKA: u32 = 7000;
/// Set when the chapter that lets the user pick their course goal (lose vs
/// maintain) opens. Until then the goal chooser stays hidden in Settings and the
/// goal defaults to weight loss.
pub const COURSE_GOAL_UNLOCKED: &str = "course_goal_unlocked";

#[derive(Serialize, Deserialize)]
struct Flag {
    key: String,
    value: bool,
    // `updated_at` (RFC3339) drives last-writer-wins when story syncs across
    // devices. `default` so flags written before sync existed still deserialize.
    #[serde(default)]
    updated_at: String,
}

/// Read a story progress flag. Defaults to `false` when not yet set.
pub async fn get_flag(key: &str) -> bool {
    db::get::<Flag>("story", key).await.map(|f| f.value).unwrap_or(false)
}

/// The local date a (set) flag was last written — used as a baseline for
/// "since this happened" counters. `None` if the flag is unset/false/undated.
async fn flag_set_date(key: &str) -> Option<chrono::NaiveDate> {
    let f = db::get::<Flag>("story", key).await?;
    if !f.value {
        return None;
    }
    chrono::DateTime::parse_from_rfc3339(&f.updated_at)
        .ok()
        .map(|dt| dt.with_timezone(&chrono::Local).date_naive())
}

/// All story flag keys currently set to `true`. Backs the DSL engine snapshot
/// (the `opened:` / `evt_closed:` / `chapter_opened:` flag sets).
pub(crate) async fn true_flags() -> HashSet<String> {
    db::list_all::<Flag>("story")
        .await
        .into_iter()
        .filter(|f| f.value)
        .map(|f| f.key)
        .collect()
}

/// Build the DSL engine snapshot from IndexedDB: the `Progress` sensor backend
/// plus the engine's `opened`/`evt_closed` sets.
///
/// MIGRATION BRIDGE: the engine reads the EXISTING milestone flags by mapping
/// each DSL id to its legacy flag, so the current trigger sites (`set_flag`) and
/// bespoke pages keep working unchanged. The `set_flag`→`emit(event)` cleanup and
/// native `opened:`/`evt_closed:` keys come in the final phase, when the bespoke
/// pages are deleted.
pub async fn engine_snapshot() -> crate::services::story_dsl::EngineSnapshot {
    let progress = gather().await;
    let f = true_flags().await;
    let has = |k: &str| f.contains(k);

    // Event/section-close DSL tasks → closed when their legacy milestone flag is set.
    const TASK_FLAG: &[(&str, &str)] = &[
        ("photos", PROGRESS_PHOTOS_TAKEN),
        ("sex", SEX_SELECTED),
        ("age", BIRTH_YEAR_SET),
        ("lang", LANGUAGE_CONFIGURED),
        ("notif", NOTIFICATION_RECEIVED),
        ("weigh_in", WEIGH_IN_REMINDER),
        ("first_weigh", FIRST_WEIGH),
        ("first_food", FIRST_FOOD_DONE),
        ("repeat_food", FOOD_REPEATED),
        ("steps_reminder", STEPS_REMINDER),
        ("first_steps", FIRST_STEPS),
        ("dish_created", COOKING_DISH_CREATED),
        ("dish_in_diary", COOKING_DISH_IN_DIARY),
        ("bones", BONES_WASTE_ENTERED),
        ("restaurant", RESTAURANT_FOOD_ENTERED),
    ];
    let evt_closed: HashSet<String> = TASK_FLAG
        .iter()
        .filter(|(_, flag)| has(flag))
        .map(|(id, _)| id.to_string())
        .collect();

    // "opened" = sections whose page the user has explicitly opened — the
    // `seen:/story/<id>` flag (set by the route watcher on navigation). This is
    // what gates section completion and chapter progression.
    let opened: HashSet<String> = f
        .iter()
        .filter_map(|k| k.strip_prefix("seen:/story/").map(str::to_string))
        .collect();

    // Per-section open date (local) from each `seen:/story/<id>` flag's timestamp —
    // the baseline for `since_open` streak/count counting (strictly-after).
    let mut section_open_date: std::collections::BTreeMap<String, chrono::NaiveDate> =
        std::collections::BTreeMap::new();
    for id in &opened {
        if let Some(d) = flag_set_date(&format!("seen:/story/{id}")).await {
            section_open_date.insert(id.clone(), d);
        }
    }

    let source_days = source_days().await;

    crate::services::story_dsl::EngineSnapshot {
        progress,
        opened,
        evt_closed,
        chapter_opened: HashSet::new(), // sticky chapter-open not persisted yet
        source_days,
        section_open_date,
    }
}

/// Parse a "YYYY-MM-DD" string to a local NaiveDate (None on bad input).
fn parse_date(d: &str) -> Option<chrono::NaiveDate> {
    chrono::NaiveDate::parse_from_str(d, "%Y-%m-%d").ok()
}

/// Build the day-source registry: for each registered source, the calendar days
/// (local NaiveDate) that satisfy its predicate. This is the ONE place a new
/// day-measurement registers — add a source closure here. The engine then reduces
/// these purely (consecutive-run for `{streak}`, distinct-count for `{count}`),
/// optionally filtered by a `since_open` baseline.
///
/// Predicates/thresholds are carried verbatim from the old per-sensor gather()
/// blocks so behaviour is identical.
async fn source_days() -> std::collections::BTreeMap<String, Vec<chrono::NaiveDate>> {
    use std::collections::BTreeMap;
    let mut out: BTreeMap<String, Vec<chrono::NaiveDate>> = BTreeMap::new();

    // Entry-date sources.
    let weight_entries = local::list_weight_entries().await;
    let weight: Vec<chrono::NaiveDate> = weight_entries
        .iter()
        .filter_map(|e| parse_date(&e.date))
        .collect();
    out.insert("weight".to_string(), weight);

    // Top-quality weigh-ins: all 5 quality conditions met (the "X/5" score = 5/5).
    // Backs the ch3 "Калория" task (weigh in at 5/5, 7 days in a row).
    let weighin_q5: Vec<chrono::NaiveDate> = weight_entries
        .iter()
        .filter(|e| e.no_water && e.no_food && e.no_wash && e.used_toilet && e.morning)
        .filter_map(|e| parse_date(&e.date))
        .collect();
    out.insert("weighin_q5".to_string(), weighin_q5);

    let step_entries = local::list_step_entries().await;
    let steps: Vec<chrono::NaiveDate> = step_entries
        .iter()
        .filter_map(|e| parse_date(&e.date))
        .collect();
    out.insert("steps".to_string(), steps);

    // Days that hit the steps planka (ch3 "Почему не уходит вес" task). The target
    // is the current Steps goal (set via the weekly card), defaulting to STEPS_PLANKA.
    let steps_target = local::steps_goal_amount().await.map(|a| a as u32).unwrap_or(STEPS_PLANKA);
    let steps_planka: Vec<chrono::NaiveDate> = step_entries
        .iter()
        .filter(|e| e.steps >= steps_target)
        .filter_map(|e| parse_date(&e.date))
        .collect();
    out.insert("steps_planka".to_string(), steps_planka);

    let diary: Vec<chrono::NaiveDate> = local::list_diary_dates()
        .await
        .iter()
        .filter_map(|d| parse_date(d))
        .collect();
    out.insert("diary".to_string(), diary);

    // Per-day facts (from each stored day summary) back the veg/protein/drink sources.
    let day_facts: Vec<(String, summary::DayFacts)> = db::list_all::<summary::Summary>("summaries")
        .await
        .into_iter()
        .filter(|s| s.id.starts_with("day:"))
        .filter_map(|s| summary::parse_day(&s.text).and_then(|d| d.facts).map(|f| (s.date, f)))
        .collect();

    // Veg/fruit: sex-specific target (≥400 g women / ≥600 g men).
    let veg_target = match profile::get_sex() {
        Some(profile::Sex::Female) => 400.0,
        Some(profile::Sex::Male) => 600.0,
        None => 600.0,
    };
    let veg: Vec<chrono::NaiveDate> = day_facts
        .iter()
        .filter(|(_, f)| f.veg_fruit_grams >= veg_target)
        .filter_map(|(d, _)| parse_date(d))
        .collect();
    out.insert("veg".to_string(), veg);

    // Protein target = 1.2 g/kg of the latest logged weight (0 if no weight).
    let protein_target_g = weight_entries
        .last()
        .map(|e| (1.2 * e.weight_kg).round() as u32)
        .unwrap_or(0);

    // Protein: days hitting the calculated target. Empty when target==0 (preserving
    // the old `only when target>0` gate → streak 0).
    let protein: Vec<chrono::NaiveDate> = if protein_target_g > 0 {
        day_facts
            .iter()
            .filter(|(_, f)| f.protein >= protein_target_g as f64)
            .filter_map(|(d, _)| parse_date(d))
            .collect()
    } else {
        Vec::new()
    };
    out.insert("protein".to_string(), protein);

    // No liquid calories: days with NO high-calorie ("liquid-calorie") drink logged.
    let no_liquid_cal: Vec<chrono::NaiveDate> = day_facts
        .iter()
        .filter(|(_, f)| !f.high_cal_drink)
        .filter_map(|(d, _)| parse_date(d))
        .collect();
    out.insert("no_liquid_cal".to_string(), no_liquid_cal);

    // Evening protein: days whose evening protein reached ≥1/3 of the daily norm.
    // Empty when target==0 (preserving the old gate → count 0). Used with COUNT.
    let evening_protein: Vec<chrono::NaiveDate> = if protein_target_g > 0 {
        let third = protein_target_g as f64 / 3.0;
        day_facts
            .iter()
            .filter(|(_, f)| f.evening_protein_g >= third)
            .filter_map(|(d, _)| parse_date(d))
            .collect()
    } else {
        Vec::new()
    };
    out.insert("evening_protein".to_string(), evening_protein);

    out
}

/// Persist a story progress flag in the IndexedDB `story` store, stamp it for
/// cross-device sync, and push in the background so progress propagates.
pub async fn set_flag(key: &str, value: bool) {
    let updated_at = chrono::Utc::now().to_rfc3339();
    db::put("story", &Flag { key: key.to_string(), value, updated_at }).await;
    sync::push_background();
}

/// Run a named `on_open` action for a story section. The closed registry the DSL
/// references; unknown names fail loud in the log. (Goal-setting actions for
/// chapters 2/3 are ported here as those sections migrate.)
pub async fn run_action(name: &str) {
    use crate::services::local;
    use api_types::{CreateGoalInput, GoalDirection, GoalPeriod, GoalUnit};
    let now = || chrono::Utc::now().to_rfc3339();
    match name {
        "arm_first_food" => {
            // Only ARM the task on open; it closes later, when food is actually
            // logged (the diary-add flow calls `fire_first_food_if_armed`). Firing
            // here would complete the task the moment the section opens.
            set_flag(FIRST_FOOD_ARMED, true).await;
            set_flag(MEAL_REMINDERS_UNLOCKED, true).await;
        }
        "unlock_meal_split" => set_flag(MEAL_SPLIT_UNLOCKED, true).await,
        "view_night_feedback" => set_flag(NIGHT_FEEDBACK_VIEWED, true).await,
        // Hidden non-track Protein goal: 1.2 g/kg of the latest weight, rounded up
        // to the nearest 10 g. No-op (and the widget shows "need weight") if there
        // are no weight entries yet.
        "set_protein_goal" => {
            if let Some(latest) = local::list_weight_entries().await.into_iter().last() {
                let target = (1.2 * latest.weight_kg).round();
                match local::list_goals().await.into_iter().find(|g| g.nutrient == "Protein") {
                    Some(mut g) => {
                        g.direction = GoalDirection::AtLeast;
                        g.amount = target;
                        g.unit = GoalUnit::G;
                        g.period = GoalPeriod::Day;
                        g.updated_at = now();
                        local::update_goal(&g).await;
                    }
                    None => {
                        local::create_goal(CreateGoalInput {
                            nutrient: "Protein".to_string(),
                            direction: GoalDirection::AtLeast,
                            amount: target,
                            unit: GoalUnit::G,
                            period: GoalPeriod::Day,
                        })
                        .await;
                    }
                }
                sync::push_background();
            }
        }
        // Veg/fruit aggregate planka: sex-specific daily target (400 g women /
        // 600 g men). Set when the section opens — joins the goals list.
        "set_veg_goal" => {
            let target = match profile::get_sex() {
                Some(profile::Sex::Female) => 400.0,
                _ => 600.0,
            };
            match local::list_goals().await.into_iter().find(|g| g.nutrient == "Овощи и фрукты") {
                Some(mut g) => {
                    g.direction = GoalDirection::AtLeast;
                    g.amount = target;
                    g.unit = GoalUnit::G;
                    g.period = GoalPeriod::Day;
                    g.updated_at = now();
                    local::update_goal(&g).await;
                }
                None => {
                    local::create_goal(CreateGoalInput {
                        nutrient: "Овощи и фрукты".to_string(),
                        direction: GoalDirection::AtLeast,
                        amount: target,
                        unit: GoalUnit::G,
                        period: GoalPeriod::Day,
                    })
                    .await;
                }
            }
            sync::push_background();
        }
        // Hidden non-track Calorie planka: avg daily kcal over the last 14 days with
        // diary entries; the trend balance decides avg (deficit) vs avg*0.95, rounded
        // to 10 kcal. No-op (widget shows "need diary") if there are no diary days.
        "set_calorie_planka" => {
            use crate::services::weight_trend::{self, DEFAULT_WINDOW_DAYS};
            if let Some(avg) = local::avg_daily_kcal(14).await {
                let weights = local::list_weight_entries().await;
                let balance = weight_trend::weight_trend(&weights, DEFAULT_WINDOW_DAYS).balance();
                let planka = local::calorie_planka(avg, balance);
                match local::list_goals().await.into_iter().find(|g| g.nutrient == "Calories") {
                    Some(mut g) => {
                        g.direction = GoalDirection::AtMost;
                        g.amount = planka;
                        g.unit = GoalUnit::Kcal;
                        g.period = GoalPeriod::Day;
                        g.updated_at = now();
                        local::update_goal(&g).await;
                    }
                    None => {
                        local::create_goal(CreateGoalInput {
                            nutrient: "Calories".to_string(),
                            direction: GoalDirection::AtMost,
                            amount: planka,
                            unit: GoalUnit::Kcal,
                            period: GoalPeriod::Day,
                        })
                        .await;
                    }
                }
                sync::push_background();
            }
        }
        _ => leptos::logging::warn!("story: unknown on_open action '{name}'"),
    }
}

/// Complete the "first food entries" task if its trigger was armed (the section
/// was opened) and it isn't done yet. Called when a food is added to the diary.
pub async fn fire_first_food_if_armed() {
    if get_flag(FIRST_FOOD_ARMED).await && !get_flag(FIRST_FOOD_DONE).await {
        set_flag(FIRST_FOOD_DONE, true).await;
    }
}

// ---------------------------------------------------------------------------
// Story progress model (shared by the Story page and the attention markers).
// ---------------------------------------------------------------------------

/// A snapshot of everything the unlock/completion rules depend on. Built once
/// from signals (the Story page) or from the DB (`gather`), then fed into the
/// pure rules below — so the page and the nav-icon attention marker can never
/// disagree about what's unlocked or done.
#[derive(Default, Clone, Debug)]
pub struct Progress {
    pub progress_photos: bool,
    pub sex_done: bool,
    pub lang_done: bool,
    pub notif_done: bool,
    pub weigh_in_on: bool,
    pub first_weigh: bool,
    pub first_food_done: bool,
    pub steps_reminder_on: bool,
    pub first_steps: bool,
    pub dish_created: bool,
    pub dish_in_diary: bool,
    pub bones_waste: bool,
    pub restaurant_food: bool,
    pub meal_split_unlocked: bool,
    pub night_feedback_viewed: bool,
    /// The calculated daily protein target in grams: 1.2 g per kg of the latest
    /// logged body weight, rounded up to 10 g (0 if no weight logged). Shown in
    /// the protein task title.
    pub protein_target_g: u32,
    pub calorie_planka_set: bool,
    pub s4_done: bool,
    pub s5_done: bool,
    pub sub_active: bool,
    pub sub_paid: bool,
}


/// Substitute the `{n}` target placeholder in a task title. The veg task shows
/// the sex-specific target (400 g women / 600 g men); the protein task shows the
/// calculated 1.2 g/kg target. Other tasks are returned unchanged. Used in BOTH
/// the per-section task list and the hub's "active tasks" list.
pub fn fill_task_target(id: &str, title: String, progress: &Progress) -> String {
    match id {
        "veg_streak" => {
            let g = match crate::services::profile::get_sex() {
                Some(crate::services::profile::Sex::Female) => 400,
                _ => 600,
            };
            title.replace("{n}", &g.to_string())
        }
        "protein_streak" => title.replace("{n}", &progress.protein_target_g.to_string()),
        _ => title,
    }
}

/// Collect the current [`Progress`] from the DB (bool milestone flags + the
/// displayed protein target + single-day bool sensors + cached subscription).
/// Consecutive-day streaks / counts are NOT here — they are derived in-engine from
/// the day-source registry (`source_days()`), filtered by section open dates.
pub async fn gather() -> Progress {
    let calorie_planka_set = local::list_goals().await.into_iter().any(|g| {
        g.nutrient == "Calories"
            && g.direction == api_types::GoalDirection::AtMost
            && g.amount > 0.0
    });

    let y = local::yesterday();
    let report_ready = local::report_ready_on(&y).await;
    let s4_done = report_ready && local::snack_logged_on(&y).await;
    // The drinks task counts a clean day only AFTER the ch2-drinks section was
    // opened, so it isn't pre-satisfied by yesterday at opening time. Baseline is
    // that section's `seen` flag (the old DRINKS_ARMED flag is gone).
    let drinks_after_open = match flag_set_date("seen:/story/ch2-drinks").await {
        Some(base) => chrono::NaiveDate::parse_from_str(&y, "%Y-%m-%d").map(|d| d > base).unwrap_or(false),
        None => false,
    };
    let s5_done = report_ready && !local::high_cal_drink_on(&y).await && drinks_after_open;

    // Protein target = 1.2 g/kg of the latest logged weight (0 if no weight). Kept
    // here because it is a displayed target (fill_task_target) — not a sensor.
    let protein_target_g = local::list_weight_entries()
        .await
        .into_iter()
        .last()
        .map(|e| (1.2 * e.weight_kg).round() as u32)
        .unwrap_or(0);

    let sub = subscription::cached();

    Progress {
        progress_photos: get_flag(PROGRESS_PHOTOS_TAKEN).await,
        sex_done: get_flag(SEX_SELECTED).await,
        lang_done: get_flag(LANGUAGE_CONFIGURED).await,
        notif_done: get_flag(NOTIFICATION_RECEIVED).await,
        weigh_in_on: get_flag(WEIGH_IN_REMINDER).await,
        first_weigh: get_flag(FIRST_WEIGH).await,
        first_food_done: get_flag(FIRST_FOOD_DONE).await,
        steps_reminder_on: get_flag(STEPS_REMINDER).await,
        first_steps: get_flag(FIRST_STEPS).await,
        dish_created: get_flag(COOKING_DISH_CREATED).await,
        dish_in_diary: get_flag(COOKING_DISH_IN_DIARY).await,
        bones_waste: get_flag(BONES_WASTE_ENTERED).await,
        restaurant_food: get_flag(RESTAURANT_FOOD_ENTERED).await,
        meal_split_unlocked: get_flag(MEAL_SPLIT_UNLOCKED).await,
        night_feedback_viewed: get_flag(NIGHT_FEEDBACK_VIEWED).await,
        protein_target_g,
        calorie_planka_set,
        s4_done,
        s5_done,
        sub_active: sub.as_ref().map(|s| s.active).unwrap_or(false),
        sub_paid: sub.as_ref().map(|s| s.is_paid()).unwrap_or(false),
    }
}

// ---------------------------------------------------------------------------
// Attention markers: "a section is unlocked but unread" / "a task just got done".
// ---------------------------------------------------------------------------

/// Story flag key marking that the user has opened a section's page.
fn seen_key(route: &str) -> String {
    format!("seen:{route}")
}
/// Story flag key marking that a completed task has been acknowledged (the user
/// has since opened the Story page).
fn ack_key(task: &str) -> String {
    format!("ack:{task}")
}

/// Every section route (`/story/<id>`) across all chapters, from the DSL.
pub fn all_section_routes() -> Vec<String> {
    story_dsl::story()
        .chapters
        .iter()
        .flat_map(|c| c.sections.iter())
        .map(|s| format!("/story/{}", s.id))
        .collect()
}

/// True if `path` is a known story-section route (`/story/<known id>`).
pub fn is_section_route(path: &str) -> bool {
    path.strip_prefix("/story/").is_some_and(|id| {
        story_dsl::story().chapters.iter().flat_map(|c| &c.sections).any(|s| s.id == id)
    })
}

/// The set of section routes the user has already opened.
pub async fn seen_routes() -> HashSet<String> {
    let mut set = HashSet::new();
    for route in all_section_routes() {
        if get_flag(&seen_key(&route)).await {
            set.insert(route);
        }
    }
    set
}

/// Mark a section's page as seen (once) — clears its "new" dot. No-op if the
/// route is unknown or already seen, so navigation doesn't spam writes/sync.
pub async fn mark_section_seen(route: &str) {
    if !is_section_route(route) {
        return;
    }
    let k = seen_key(route);
    if !get_flag(&k).await {
        set_flag(&k, true).await;
    }
}

/// Persist a flag without an immediate sync push (used for batch writes).
async fn put_flag(key: &str, value: bool) {
    let updated_at = chrono::Utc::now().to_rfc3339();
    db::put("story", &Flag { key: key.to_string(), value, updated_at }).await;
}

/// Acknowledge every currently-completed task — clears the "task done" marker.
/// Called when the user opens the Story page. Writes in one batch, pushes once.
pub async fn ack_done_tasks() {
    let snap = engine_snapshot().await;
    let story = story_dsl::story();
    let e = Engine::new(story, &snap);
    let mut wrote = false;
    for t in &story.tasks {
        if e.task_closed(&t.id) {
            let k = ack_key(&t.id);
            if !get_flag(&k).await {
                put_flag(&k, true).await;
                wrote = true;
            }
        }
    }
    if wrote {
        sync::push_background();
    }
}

/// Whether the Story deserves an attention marker right now.
#[derive(Default, Clone, Copy)]
pub struct Attention {
    /// A section is unlocked (openable) but the user hasn't opened it yet.
    pub unread_section: bool,
    /// A task is completed but hasn't been acknowledged (Story not yet opened since).
    pub unacked_task: bool,
}

impl Attention {
    pub fn any(&self) -> bool {
        self.unread_section || self.unacked_task
    }
}

/// Compute the current attention state from the DB.
pub async fn attention() -> Attention {
    let snap = engine_snapshot().await;
    let story = story_dsl::story();
    let e = Engine::new(story, &snap);
    let seen = seen_routes().await;

    // A section is "unread" when it's unlocked but its page hasn't been opened.
    let mut unread_section = false;
    for ch in &story.chapters {
        if !e.chapter_open(ch) {
            continue;
        }
        for (i, sec) in ch.sections.iter().enumerate() {
            if e.section_unlocked(ch, i) && !seen.contains(&format!("/story/{}", sec.id)) {
                unread_section = true;
            }
        }
    }

    // A task is "unacked" when it's closed but the user hasn't opened the Story since.
    let mut unacked_task = false;
    for t in &story.tasks {
        if e.task_closed(&t.id) && !get_flag(&ack_key(&t.id)).await {
            unacked_task = true;
            break;
        }
    }

    Attention { unread_section, unacked_task }
}

thread_local! {
    // Reactive "the Story has something new" flag, shown as a dot on the nav-story
    // icon. Created at the ROOT via init_attention() (like db::version / update),
    // never lazily inside a reactive closure.
    static ATTENTION: RefCell<Option<RwSignal<bool>>> = const { RefCell::new(None) };
}

/// Create the shared attention flag in the root scope. Call once from main().
pub fn init_attention() {
    ATTENTION.with(|c| {
        if c.borrow().is_none() {
            *c.borrow_mut() = Some(create_rw_signal(false));
        }
    });
}

/// Reactive flag: true when the Story has an unread section or an unacked task.
pub fn attention_signal() -> RwSignal<bool> {
    ATTENTION.with(|c| c.borrow().expect("story::init_attention() must run first"))
}

/// Recompute the attention flag from the DB (fire-and-forget).
pub fn refresh_attention() {
    spawn_local(async move {
        attention_signal().set(attention().await.any());
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn section_routes_are_recognised() {
        // Engine/unlock logic is tested in story_dsl; here just the route mapping.
        assert!(is_section_route("/story/intro"));
        assert!(is_section_route("/story/ch3-reward"));
        assert!(!is_section_route("/diary"));
        assert!(!is_section_route("/paywall"));
    }
}

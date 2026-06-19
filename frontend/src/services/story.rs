use std::cell::RefCell;
use std::collections::HashSet;

use leptos::*;
use serde::{Deserialize, Serialize};

use crate::services::{db, local, subscription, sync};

/// Sections of chapter 1: (emoji icon, i18n label key, route once unlocked).
pub const CH1_SECTIONS: [(&str, &str, Option<&str>); 9] = [
    ("\u{27a1}\u{fe0f}", "story.ch1.intro", Some("/story/intro")),
    ("\u{2699}\u{fe0f}", "story.ch1.setup", Some("/story/setup")),
    ("\u{1f4b0}", "story.ch1.accounting", Some("/story/accounting")),
    ("\u{1f37d}\u{fe0f}", "story.ch1.first_food", Some("/story/first-food")),
    ("\u{1f6b6}", "story.ch1.activity", Some("/story/activity")),
    ("\u{1f468}\u{200d}\u{1f373}", "story.ch1.cooking", Some("/story/cooking")),
    ("\u{1f9b4}", "story.ch1.bones", Some("/story/bones")),
    ("\u{1f389}", "story.ch1.restaurant", Some("/story/restaurant")),
    ("\u{1f513}", "story.ch1.next", Some("/story/next")),
];

/// Sections of chapter 2 «Appetite».
pub const CH2_SECTIONS: [(&str, &str, Option<&str>); 7] = [
    ("\u{26a0}\u{fe0f}", "story.ch2.s1", Some("/story/ch2-mistake")),
    ("\u{1f966}", "story.ch2.s2", Some("/story/ch2-veg")),
    ("\u{1f357}", "story.ch2.s3", Some("/story/ch2-protein")),
    ("\u{1f37f}", "story.ch2.s4", Some("/story/ch2-snack")),
    ("\u{1f964}", "story.ch2.s5", Some("/story/ch2-drinks")),
    ("\u{1f37d}\u{fe0f}", "story.ch2.s6", Some("/story/ch2-meals")),
    ("\u{1f319}", "story.ch2.s7", Some("/story/ch2-night")),
];

/// Sections of chapter 3 «Why lose weight?». s1 carries the calorie-planka
/// mechanic; the rest are read-only prose.
pub const CH3_SECTIONS: [(&str, &str, Option<&str>); 6] = [
    ("\u{1f525}", "story.ch3.s1", Some("/story/ch3-fat")),
    ("\u{1fa7a}", "story.ch3.aesthetics", Some("/story/ch3-aesthetics")),
    ("\u{1fa9e}", "story.ch3.s2", Some("/story/ch3-beauty")),
    ("\u{1f4c9}", "story.ch3.s3", Some("/story/ch3-minimum")),
    ("\u{2696}\u{fe0f}", "story.ch3.s4", Some("/story/ch3-lean")),
    ("\u{1f331}", "story.ch3.s5", Some("/story/ch3-lifestyle")),
];

/// Chapter 3 opens once the user has tracked the food diary for this many days.
pub const CH3_MIN_DIARY_DAYS: u32 = 7;

/// Stable keys for the chapter progress tasks, aligned 1:1 (same order) with
/// [`Progress::tasks`]. Used to persist per-task "acknowledged" flags.
pub const TASK_KEYS: [&str; 20] = [
    "progress_photos", "sex", "lang", "notif", "weigh_in",
    "first_weigh", "weight_streak", "first_food",
    "steps_reminder", "first_steps", "steps_streak",
    "dish_created", "dish_in_diary", "bones", "restaurant",
    "sub_paid", "diary_streak", "s4", "s5", "night",
];

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
/// Chapter 2 / s6: set when the meal-split section is opened. While set, the
/// diary page groups the day's entries by derived meal instead of a flat list.
pub const MEAL_SPLIT_UNLOCKED: &str = "meal_split_unlocked";
/// Chapter 2 / s7: set when the "eating at night" section is opened and the
/// user views today's evening feedback. Completes that section's task.
pub const NIGHT_FEEDBACK_VIEWED: &str = "night_feedback_viewed";

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
        ("lang", LANGUAGE_CONFIGURED),
        ("notif", NOTIFICATION_RECEIVED),
        ("weigh_in", WEIGH_IN_REMINDER),
        ("first_weigh", FIRST_WEIGH),
        ("first_food", FIRST_FOOD_DONE),
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

    // `{section_opened: id}` conditions (armed first-food; meals/night complete-on-open).
    const SECTION_FLAG: &[(&str, &str)] = &[
        ("first-food", FIRST_FOOD_ARMED),
        ("ch2-meals", MEAL_SPLIT_UNLOCKED),
        ("ch2-night", NIGHT_FEEDBACK_VIEWED),
    ];
    let opened: HashSet<String> = SECTION_FLAG
        .iter()
        .filter(|(_, flag)| has(flag))
        .map(|(id, _)| id.to_string())
        .collect();

    crate::services::story_dsl::EngineSnapshot {
        progress,
        opened,
        evt_closed,
        chapter_opened: HashSet::new(), // live-evaluated during the bridge
    }
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
    match name {
        "arm_first_food" => {
            set_flag(FIRST_FOOD_ARMED, true).await;
            set_flag(MEAL_REMINDERS_UNLOCKED, true).await;
            fire_first_food_if_armed().await;
        }
        "unlock_meal_split" => set_flag(MEAL_SPLIT_UNLOCKED, true).await,
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

/// Longest run of consecutive calendar days present in `dates` (each "YYYY-MM-DD").
pub fn consecutive_day_streak(dates: &[String]) -> u32 {
    let mut days: Vec<chrono::NaiveDate> = dates
        .iter()
        .filter_map(|d| chrono::NaiveDate::parse_from_str(d, "%Y-%m-%d").ok())
        .collect();
    days.sort();
    days.dedup();

    let mut best = 0u32;
    let mut cur = 0u32;
    let mut prev: Option<chrono::NaiveDate> = None;
    for d in days {
        cur = match prev {
            Some(p) if d == p + chrono::Duration::days(1) => cur + 1,
            _ => 1,
        };
        best = best.max(cur);
        prev = Some(d);
    }
    best
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
    pub weight_streak: u32,
    pub steps_streak: u32,
    pub diary_streak: u32,
    pub diary_days: u32,
    pub calorie_planka_set: bool,
    pub s4_done: bool,
    pub s5_done: bool,
    pub sub_active: bool,
    pub sub_paid: bool,
}

impl Progress {
    /// Chapter 1: the task(s) that open the next section are done.
    pub fn ch1_completed(&self, i: usize) -> bool {
        match i {
            0 => self.progress_photos,
            1 => self.lang_done && self.notif_done,
            2 => self.weigh_in_on,
            3 => self.first_food_done,
            4 => self.first_steps,
            5 => self.dish_created && self.dish_in_diary,
            6 => self.bones_waste,
            7 => self.restaurant_food,
            8 => self.sub_paid,
            _ => false,
        }
    }
    /// Chapter 1 unlocking is cumulative: open only if every earlier section is done.
    pub fn ch1_unlocked(&self, i: usize) -> bool {
        (0..i).all(|j| self.ch1_completed(j))
    }

    /// Chapter 2 opens after a 7-day weigh-in streak AND an active subscription.
    pub fn chapter2_unlocked(&self) -> bool {
        self.weight_streak >= 7 && self.sub_active
    }
    pub fn ch2_completed(&self, i: usize) -> bool {
        match i {
            0 => self.diary_streak >= 7,
            3 => self.s4_done,
            4 => self.s5_done,
            5 => self.meal_split_unlocked,
            6 => self.night_feedback_viewed,
            _ => true, // s2 / s3: post-factum / goal — no gate
        }
    }
    pub fn ch2_unlocked(&self, i: usize) -> bool {
        self.chapter2_unlocked() && (0..i).all(|j| self.ch2_completed(j))
    }

    /// Chapter 3 opens once the diary has been tracked for several distinct days.
    pub fn chapter3_unlocked(&self) -> bool {
        self.diary_days >= CH3_MIN_DIARY_DAYS
    }
    pub fn ch3_completed(&self, i: usize) -> bool {
        match i {
            0 => self.calorie_planka_set,
            _ => true, // s2..s5: read-only prose
        }
    }
    pub fn ch3_unlocked(&self, i: usize) -> bool {
        self.chapter3_unlocked() && (0..i).all(|j| self.ch3_completed(j))
    }

    /// All chapter progress tasks, in the same order as [`TASK_KEYS`].
    pub fn tasks(&self) -> [bool; 20] {
        [
            self.progress_photos, self.sex_done, self.lang_done, self.notif_done, self.weigh_in_on,
            self.first_weigh, self.weight_streak >= 7, self.first_food_done,
            self.steps_reminder_on, self.first_steps, self.steps_streak >= 7,
            self.dish_created, self.dish_in_diary, self.bones_waste, self.restaurant_food,
            self.sub_paid, self.diary_streak >= 7, self.s4_done, self.s5_done, self.night_feedback_viewed,
        ]
    }
}

/// Collect the current [`Progress`] from the DB (flags + derived streaks +
/// cached subscription). Mirrors the per-signal effects on the Story page.
pub async fn gather() -> Progress {
    let weight_dates: Vec<String> =
        local::list_weight_entries().await.into_iter().map(|e| e.date).collect();
    let steps_dates: Vec<String> =
        local::list_step_entries().await.into_iter().map(|e| e.date).collect();
    let mut diary_dates = local::list_diary_dates().await;
    let diary_streak = consecutive_day_streak(&diary_dates);
    diary_dates.sort();
    diary_dates.dedup();
    let diary_days = diary_dates.len() as u32;

    let calorie_planka_set = local::list_goals().await.into_iter().any(|g| {
        g.nutrient == "Calories"
            && g.direction == api_types::GoalDirection::AtMost
            && g.amount > 0.0
    });

    let y = local::yesterday();
    let report_ready = local::report_ready_on(&y).await;
    let s4_done = report_ready && local::snack_logged_on(&y).await;
    let s5_done = report_ready && !local::high_cal_drink_on(&y).await;

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
        weight_streak: consecutive_day_streak(&weight_dates),
        steps_streak: consecutive_day_streak(&steps_dates),
        diary_streak,
        diary_days,
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

/// Every section route across all chapters, for "unread section" detection.
pub fn all_section_routes() -> Vec<&'static str> {
    CH1_SECTIONS
        .iter()
        .chain(CH2_SECTIONS.iter())
        .chain(CH3_SECTIONS.iter())
        .filter_map(|(_, _, r)| *r)
        .collect()
}

/// True if `path` is a known story-section route.
pub fn is_section_route(path: &str) -> bool {
    all_section_routes().contains(&path)
}

/// The set of section routes the user has already opened.
pub async fn seen_routes() -> HashSet<String> {
    let mut set = HashSet::new();
    for route in all_section_routes() {
        if get_flag(&seen_key(route)).await {
            set.insert(route.to_string());
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
    let p = gather().await;
    let tasks = p.tasks();
    let mut wrote = false;
    for (i, done) in tasks.iter().enumerate() {
        if *done {
            let k = ack_key(TASK_KEYS[i]);
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
    let p = gather().await;
    let seen = seen_routes().await;

    let unread_in = |sections: &[(&str, &str, Option<&str>)], unlocked: &dyn Fn(usize) -> bool| {
        sections.iter().enumerate().any(|(i, (_, _, route))| {
            matches!(route, Some(r) if unlocked(i) && !seen.contains(*r))
        })
    };
    let unread_section = unread_in(&CH1_SECTIONS, &|i| p.ch1_unlocked(i))
        || unread_in(&CH2_SECTIONS, &|i| p.ch2_unlocked(i))
        || unread_in(&CH3_SECTIONS, &|i| p.ch3_unlocked(i));

    let tasks = p.tasks();
    let mut unacked_task = false;
    for (i, done) in tasks.iter().enumerate() {
        if *done && !get_flag(&ack_key(TASK_KEYS[i])).await {
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

    fn full() -> Progress {
        Progress {
            progress_photos: true, sex_done: true, lang_done: true, notif_done: true,
            weigh_in_on: true, first_weigh: true, first_food_done: true,
            steps_reminder_on: true, first_steps: true, dish_created: true, dish_in_diary: true,
            bones_waste: true, restaurant_food: true, meal_split_unlocked: true,
            night_feedback_viewed: true, weight_streak: 7, steps_streak: 7,
            diary_streak: 7, diary_days: 7, calorie_planka_set: true, s4_done: true, s5_done: true,
            sub_active: true, sub_paid: true,
        }
    }

    #[test]
    fn ch1_chain_is_cumulative() {
        let mut p = Progress::default();
        assert!(p.ch1_unlocked(0)); // first is always open
        assert!(!p.ch1_unlocked(1)); // locked until section 0 done
        p.progress_photos = true;
        assert!(p.ch1_unlocked(1));
        assert!(!p.ch1_unlocked(2)); // needs lang+notif
    }

    #[test]
    fn chapter2_needs_streak_and_subscription() {
        let mut p = Progress::default();
        p.weight_streak = 7;
        assert!(!p.chapter2_unlocked()); // no subscription
        p.sub_active = true;
        assert!(p.chapter2_unlocked());
    }

    #[test]
    fn chapter3_gates_on_diary_days() {
        let mut p = Progress::default();
        p.diary_days = 6;
        assert!(!p.chapter3_unlocked());
        p.diary_days = 7;
        assert!(p.chapter3_unlocked());
    }

    #[test]
    fn tasks_count_matches_keys_len() {
        assert_eq!(full().tasks().len(), TASK_KEYS.len());
        assert!(full().tasks().iter().all(|&d| d));
        assert_eq!(Progress::default().tasks().iter().filter(|&&d| d).count(), 0);
    }

    #[test]
    fn section_routes_are_recognised() {
        assert!(is_section_route("/story/intro"));
        assert!(is_section_route("/story/ch3-lifestyle"));
        assert!(!is_section_route("/diary"));
        assert!(!is_section_route("/paywall"));
    }
}

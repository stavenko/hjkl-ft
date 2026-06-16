use serde::{Deserialize, Serialize};

use crate::services::db;

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

#[derive(Serialize, Deserialize)]
struct Flag {
    key: String,
    value: bool,
}

/// Read a story progress flag. Defaults to `false` when not yet set.
pub async fn get_flag(key: &str) -> bool {
    db::get::<Flag>("story", key).await.map(|f| f.value).unwrap_or(false)
}

/// Persist a story progress flag in the IndexedDB `story` store.
pub async fn set_flag(key: &str, value: bool) {
    db::put("story", &Flag { key: key.to_string(), value }).await;
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

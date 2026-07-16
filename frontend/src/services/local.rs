use std::collections::BTreeMap;

use api_types::*;

use super::db;

fn now() -> String {
    chrono::Utc::now().to_rfc3339()
}

fn new_id() -> String {
    uuid::Uuid::now_v7().to_string()
}

/// Hour (local) at which a new logical/diary day begins. Entries logged BEFORE
/// this hour belong to the PREVIOUS day, so a 01:00 night snack lands on the day
/// it really belongs to instead of opening a fresh calendar day (which used to
/// swallow the following morning's breakfast into "ночной перекус"). The diary,
/// its date navigation and daily aggregations all pivot on this.
pub const DAY_START_HOUR: i64 = 4;

/// The current logical day: the calendar date shifted back by [`DAY_START_HOUR`],
/// so the day flips at 04:00 local rather than at midnight.
pub fn today_date() -> chrono::NaiveDate {
    (chrono::Local::now() - chrono::Duration::hours(DAY_START_HOUR)).date_naive()
}

/// The current logical day as "YYYY-MM-DD" (see [`today_date`]).
pub fn today() -> String {
    today_date().format("%Y-%m-%d").to_string()
}

fn time_now() -> String {
    chrono::Local::now().format("%H:%M").to_string()
}

// --- Foods ---

pub async fn list_foods() -> Vec<Food> {
    db::list_all("foods").await
}

pub async fn archive_food(id: &str, archived: bool) -> Option<Food> {
    let mut food: Food = db::get("foods", id).await?;
    food.archived = archived;
    food.updated_at = now();
    db::put("foods", &food).await;
    Some(food)
}

// --- Diary ---

pub async fn latest_diary_time_per_food() -> std::collections::BTreeMap<String, String> {
    let entries: Vec<DiaryEntry> = db::list_all("diary").await;
    let mut map = std::collections::BTreeMap::new();
    for e in entries {
        if e.deleted { continue; }
        map.entry(e.food_id)
            .and_modify(|existing: &mut String| {
                if e.created_at > *existing { *existing = e.created_at.clone(); }
            })
            .or_insert(e.created_at);
    }
    map
}

pub async fn list_diary_range(dates: &[String]) -> Vec<DiaryEntry> {
    let mut all = Vec::new();
    for d in dates {
        let entries: Vec<DiaryEntry> = db::list_by_index("diary", "date", d).await;
        all.extend(entries.into_iter().filter(|e| !e.deleted));
    }
    all
}

pub async fn list_diary(date: &str) -> Vec<DiaryEntry> {
    let entries: Vec<DiaryEntry> = db::list_by_index("diary", "date", date).await;
    entries.into_iter().filter(|e| !e.deleted).collect()
}

/// All distinct dates with at least one non-deleted diary entry.
pub async fn list_diary_dates() -> Vec<String> {
    let entries: Vec<DiaryEntry> = db::list_all("diary").await;
    entries
        .into_iter()
        .filter(|e| !e.deleted)
        .map(|e| e.date)
        .collect()
}

/// Per-day total effective kcal over the last `window_days` calendar days ending
/// today, OLDEST first (`(date, kcal)`). Days with no diary entries are `0.0`
/// (drawn as an empty slot). Same per-entry formula as [`avg_daily_kcal`] — honours
/// waste and the restaurant surcharge — so the chart matches the diary totals.
pub async fn daily_kcal_series(window_days: i64) -> Vec<(String, f64)> {
    // ONE read of the whole diary + in-memory bucketing (not `window_days`
    // separate index queries) so the chart loads in a single frame.
    let all: Vec<DiaryEntry> = db::list_all("diary").await;
    // Only the foods actually referenced by these entries (a slice, not the table).
    let foods = foods_by_ids(all.iter().filter(|e| !e.deleted).map(|e| e.food_id.clone())).await;
    let mut by_date: std::collections::HashMap<String, f64> = std::collections::HashMap::new();
    for e in &all {
        if e.deleted {
            continue;
        }
        if let Some(food) = foods.get(&e.food_id) {
            let eaten = (e.grams - e.waste_grams).max(0.0);
            *by_date.entry(e.date.clone()).or_insert(0.0) += food.effective_kcal() * eaten / 100.0;
        }
    }
    let today = chrono::Local::now().date_naive();
    (0..window_days)
        .rev()
        .map(|i| {
            let d = (today - chrono::Duration::days(i)).format("%Y-%m-%d").to_string();
            let kc = by_date.get(&d).copied().unwrap_or(0.0);
            (d, kc)
        })
        .collect()
}

/// Total effective kcal eaten on `date` (same per-entry formula as the diary).
pub async fn kcal_on(date: &str) -> f64 {
    let entries = list_diary(date).await;
    let foods = foods_by_ids(entries.iter().map(|e| e.food_id.clone())).await;
    entries
        .iter()
        .filter_map(|e| {
            foods
                .get(&e.food_id)
                .map(|food| food.effective_kcal() * (e.grams - e.waste_grams).max(0.0) / 100.0)
        })
        .sum()
}

/// Average daily effective kcal over the last `window_days` calendar days,
/// counting ONLY days that have diary entries. Per-day kcal is the sum of each
/// entry's `effective_kcal() * (grams - waste_grams).max(0) / 100` — exactly how
/// the diary/summary computes calories (honouring waste and the restaurant
/// surcharge). Returns `None` when no day in the window has any diary entry.
pub async fn avg_daily_kcal(window_days: i64) -> Option<f64> {
    let today = chrono::Local::now().date_naive();
    // Only COMPLETED days: today is still in progress (maybe just breakfast so
    // far), and averaging a partial day would drag the mean down. So the window
    // is the `window_days` days ENDING YESTERDAY (today-1 … today-window_days).
    let dates: Vec<String> = (1..=window_days)
        .map(|i| (today - chrono::Duration::days(i)).format("%Y-%m-%d").to_string())
        .collect();

    // Diaries for the days that have entries, then only the foods they reference.
    let mut diaries: Vec<Vec<DiaryEntry>> = Vec::new();
    for d in &dates {
        let diary = list_diary(d).await;
        if !diary.is_empty() {
            diaries.push(diary);
        }
    }
    if diaries.is_empty() {
        return None;
    }
    let foods = foods_by_ids(diaries.iter().flatten().map(|e| e.food_id.clone())).await;

    let day_totals: Vec<f64> = diaries
        .iter()
        .map(|diary| {
            diary
                .iter()
                .filter_map(|e| {
                    foods
                        .get(&e.food_id)
                        .map(|food| food.effective_kcal() * (e.grams - e.waste_grams).max(0.0) / 100.0)
                })
                .sum()
        })
        .collect();

    let sum: f64 = day_totals.iter().sum();
    Some(sum / day_totals.len() as f64)
}

/// Compute the daily calorie planka from the average daily kcal and the weight
/// balance. In a deficit the planka is the average itself; for maintenance or a
/// surplus it is 5% below the average. The result is rounded to the nearest
/// 50 kcal. Pure (no I/O) so it is unit-testable.
pub fn calorie_planka(avg_kcal: f64, balance: crate::services::weight_trend::BalanceState) -> f64 {
    use crate::services::weight_trend::BalanceState;
    let raw = if balance == BalanceState::Deficit { avg_kcal } else { avg_kcal * 0.95 };
    (raw / 50.0).round() * 50.0
}

/// The suggested daily calorie planka shown (and accepted) in ch3: average intake
/// over the last 7 COMPLETED days (today excluded — it's still in progress),
/// adjusted for the current weight balance via [`calorie_planka`] (deficit → keep;
/// maintenance/surplus → −5%, rounded to 50). `None` when there are no logged days
/// yet. Single source of truth so the widget's displayed figure and the value it
/// accepts cannot drift apart.
pub async fn calorie_planka_suggestion() -> Option<f64> {
    use crate::services::weight_trend::{self, DEFAULT_WINDOW_DAYS};
    let avg = avg_daily_kcal(7).await?;
    let balance = weight_trend::weight_trend(&list_weight_entries().await, DEFAULT_WINDOW_DAYS).balance();
    Some(calorie_planka(avg, balance))
}

/// The currently-set daily calorie planka (the `Calories`/`AtMost` goal), if any.
pub async fn calorie_goal_amount() -> Option<f64> {
    list_goals()
        .await
        .into_iter()
        .find(|g| g.nutrient == "Calories" && g.direction == GoalDirection::AtMost && g.amount > 0.0)
        .map(|g| g.amount)
}

/// Progress toward the "one week of observations" the planka needs: for each of
/// food / weight / steps, how many distinct days have an entry, capped at 7.
///
/// FOOD counts only the 7 COMPLETED days (yesterday … 7 days ago). Today is a
/// PARTIAL day and is deliberately EXCLUDED — it's exactly the window the calorie
/// planka averages over, so the "calculate" button only unlocks once there are
/// seven FULL days of food (a single item logged today must NOT complete the week).
///
/// WEIGHT/STEPS use the last 8 days (today included). Why 8, not 7: weight is a
/// TODAY morning measurement while steps are logged for the COMPLETED previous day —
/// so a diligent user's steps trail weight by a day, and the extra day absorbs that
/// one-day offset so consecutive logging still reaches 7/7 for both.
pub async fn progress_week_counts() -> (u32, u32, u32) {
    let today = chrono::Local::now().date_naive();
    // Food: the 7 completed days ending yesterday (today excluded).
    let food_window: std::collections::BTreeSet<chrono::NaiveDate> =
        (1..=7).map(|i| today - chrono::Duration::days(i)).collect();
    // Weight/steps: the last 8 days (today included).
    let ws_window: std::collections::BTreeSet<chrono::NaiveDate> =
        (0..8).map(|i| today - chrono::Duration::days(i)).collect();
    let count = |dates: &[String], window: &std::collections::BTreeSet<chrono::NaiveDate>| -> u32 {
        dates
            .iter()
            .filter_map(|d| chrono::NaiveDate::parse_from_str(d, "%Y-%m-%d").ok())
            .filter(|d| window.contains(d))
            .collect::<std::collections::BTreeSet<_>>()
            .len()
            .min(7) as u32
    };
    let food = count(&list_diary_dates().await, &food_window);
    let weight_dates = list_weight_entries().await.into_iter().map(|e| e.date).collect::<Vec<_>>();
    let steps_dates = list_step_entries().await.into_iter().map(|e| e.date).collect::<Vec<_>>();
    let weight = count(&weight_dates, &ws_window);
    let steps = count(&steps_dates, &ws_window);
    (food, weight, steps)
}

// --- Chapter 2 detection helpers ---
//
// Case-insensitive substring name-matching on `food.name.to_lowercase()`. These
// back the chapter-2 section tasks and the daily report's per-day facts.

/// Substrings that mark a DRINK food (chapter 2 / s5).
const DRINK_SUBSTRINGS: &[&str] = &[
    "сок", "газиров", "кола", "лимонад", "морс", "квас", "компот", "нектар",
    "энергетик", "пепси", "фанта", "спрайт", "cola", "pepsi", "sprite", "fanta",
];

fn name_matches(name: &str, needles: &[&str]) -> bool {
    let lower = name.to_lowercase();
    needles.iter().any(|n| lower.contains(n))
}

/// True if `food` is a low-calorie snack — by the cached AI tag (language-
/// independent). `None` (not yet classified) counts as not-a-snack until tagged
/// in the background by the `classify` queue. See [`cache_food_tags`].
pub fn is_snack_food(food: &Food) -> bool {
    food.is_snack == Some(true)
}

/// True if `food` is a vegetable / fruit — by the cached AI tag. `None` counts as
/// not-veg-fruit until classified in the background.
pub fn is_veg_fruit_food(food: &Food) -> bool {
    food.is_veg_fruit == Some(true)
}

/// True if `food` is eggs / an egg product — by the cached AI tag.
pub fn is_egg_food(food: &Food) -> bool {
    food.is_egg == Some(true)
}

/// True if `food` is red / processed meat — by the cached AI tag.
pub fn is_red_meat_food(food: &Food) -> bool {
    food.is_red_meat == Some(true)
}

/// Persist AI category verdicts onto foods by id, then push once so the tags
/// propagate across devices. Written by the background `classify` queue as soon as
/// a food is logged; foods not found are skipped.
pub async fn cache_food_tags(verdicts: &[(String, crate::services::ai::FoodTags)]) {
    if verdicts.is_empty() {
        return;
    }
    for (id, tags) in verdicts {
        if let Some(mut food) = db::get::<Food>("foods", id).await {
            food.is_snack = Some(tags.snack);
            food.is_liquid_cal = Some(tags.liquid_cal);
            food.is_veg_fruit = Some(tags.veg_fruit);
            food.is_egg = Some(tags.egg);
            food.is_red_meat = Some(tags.red_meat);
            food.updated_at = now();
            db::put("foods", &food).await;
            // Tags (veg/fruit etc.) changed → only the days THIS food was eaten on
            // need re-summarizing.
            crate::services::indicators::invalidate_food(id).await;
        }
    }
    crate::services::sync::push_background();
}

/// Merge background-enriched nutrient values (name → per-100g amount, canonical
/// unit) into a food and push. Written by the background enricher; overwrites the
/// same keys idempotently.
pub async fn cache_food_nutrients(id: &str, values: BTreeMap<String, f64>) {
    if values.is_empty() {
        return;
    }
    if let Some(mut food) = db::get::<Food>("foods", id).await {
        for (k, v) in values {
            food.nutrients.insert(k, v);
        }
        food.updated_at = now();
        db::put("foods", &food).await;
        // Nutrient values changed → only the days this food was eaten on need
        // re-summarizing.
        crate::services::indicators::invalidate_food(id).await;
        crate::services::sync::push_background();
    }
}

/// True if `food` is a drink by name.
pub fn is_drink_food(food: &Food) -> bool {
    name_matches(&food.name, DRINK_SUBSTRINGS)
}

/// A drink is HIGH-CAL ("liquid calories"). Prefers the cached AI tag
/// (`is_liquid_cal`); for foods not yet classified, falls back to the old
/// name+kcal heuristic (drink name AND per-100g kcal > 10) so behaviour degrades
/// gracefully before the background queue tags it.
pub fn is_high_cal_drink(food: &Food) -> bool {
    match food.is_liquid_cal {
        Some(v) => v,
        None => is_drink_food(food) && food.kcal > 10.0,
    }
}

/// A drink is ZERO-CAL if its per-100g kcal <= 5.
pub fn is_zero_cal_drink(food: &Food) -> bool {
    is_drink_food(food) && food.kcal <= 5.0
}

/// Fetch ONLY the foods with these ids (deduped), each by primary key — a bounded
/// slice, never the whole `foods` table. Missing ids are skipped. Every per-day
/// aggregate loads just the handful of foods its own diary slice references, so
/// cost scales with what was eaten that day, not with the size of the foods table.
async fn foods_by_ids(ids: impl IntoIterator<Item = String>) -> BTreeMap<String, Food> {
    let unique: std::collections::HashSet<String> = ids.into_iter().collect();
    let mut map = BTreeMap::new();
    for id in unique {
        if let Some(f) = db::get::<Food>("foods", &id).await {
            map.insert(id, f);
        }
    }
    map
}

/// True if the diary for `date` contains at least one SNACK food.
pub async fn snack_logged_on(date: &str) -> bool {
    let entries = list_diary(date).await;
    let foods = foods_by_ids(entries.iter().map(|e| e.food_id.clone())).await;
    entries.iter().any(|e| foods.get(&e.food_id).map_or(false, is_snack_food))
}

/// True if the diary for `date` contains at least one HIGH-CAL drink.
pub async fn high_cal_drink_on(date: &str) -> bool {
    let entries = list_diary(date).await;
    let foods = foods_by_ids(entries.iter().map(|e| e.food_id.clone())).await;
    entries.iter().any(|e| foods.get(&e.food_id).map_or(false, is_high_cal_drink))
}

/// True if the diary for `date` contains at least one ZERO-CAL drink.
pub async fn zero_cal_drink_on(date: &str) -> bool {
    let entries = list_diary(date).await;
    let foods = foods_by_ids(entries.iter().map(|e| e.food_id.clone())).await;
    entries.iter().any(|e| foods.get(&e.food_id).map_or(false, is_zero_cal_drink))
}

/// Evening protein (grams) on `date`: sum of `protein * eaten_grams / 100`
/// (honouring waste, like the diary) over entries whose derived meal bucket is
/// Dinner or NightSnack.
pub async fn evening_protein_on(date: &str) -> f64 {
    let diary = list_diary(date).await;
    let foods = foods_by_ids(diary.iter().map(|e| e.food_id.clone())).await;
    let groups = crate::services::meal_split::group_by_meal(&diary);
    use crate::services::meal_split::MealType;
    let mut total = 0.0;
    for g in groups {
        if g.meal != MealType::Dinner && g.meal != MealType::NightSnack {
            continue;
        }
        for e in &g.entries {
            if let Some(food) = foods.get(&e.food_id) {
                let eaten = (e.grams - e.waste_grams).max(0.0);
                total += food.protein * eaten / 100.0;
            }
        }
    }
    total
}

/// EATEN grams of foods matching `selector` on `date`, honouring waste and
/// **expanding recipes by composition**: for a logged dish (a food with a
/// `recipe_id`), each matching ingredient contributes `ing.grams × eaten /
/// recipe.total_grams` — so e.g. the egg content of 200 g of a cake is counted from
/// the recipe's egg ingredient, not from classifying the whole cake. Plain foods
/// contribute their full eaten grams when they match. Ingredients are treated as
/// leaf foods (no nested-recipe recursion in v1).
pub async fn food_tag_grams_on(date: &str, selector: impl Fn(&Food) -> bool) -> f64 {
    let entries = list_diary(date).await;
    // The day's own foods (needed to know which entries are recipe dishes).
    let mut foods = foods_by_ids(entries.iter().map(|e| e.food_id.clone())).await;

    // Only the recipes those dishes reference, and only their ingredients (by
    // index) — never the whole recipes/ingredients tables.
    let recipe_ids: std::collections::HashSet<String> =
        foods.values().filter_map(|f| f.recipe_id.clone()).collect();
    let mut recipes: BTreeMap<String, Recipe> = BTreeMap::new();
    let mut ings_by_recipe: BTreeMap<String, Vec<RecipeIngredient>> = BTreeMap::new();
    for rid in &recipe_ids {
        if let Some(r) = db::get::<Recipe>("recipes", rid).await {
            recipes.insert(rid.clone(), r);
        }
        ings_by_recipe.insert(
            rid.clone(),
            db::list_by_index("recipe_ingredients", "recipe_id", rid).await,
        );
    }
    // Leaf ingredient foods, so the selector can be evaluated on them too.
    let ing_ids: Vec<String> = ings_by_recipe.values().flatten().map(|i| i.food_id.clone()).collect();
    for (id, f) in foods_by_ids(ing_ids).await {
        foods.entry(id).or_insert(f);
    }

    let mut total = 0.0;
    for e in entries {
        let Some(food) = foods.get(&e.food_id) else { continue };
        let eaten = (e.grams - e.waste_grams).max(0.0);
        if eaten <= 0.0 {
            continue;
        }
        match food.recipe_id.as_ref().and_then(|rid| recipes.get(rid)) {
            // Logged dish → attribute matching ingredients by their share of the
            // finished (cooked) weight.
            Some(recipe) if recipe.total_grams.unwrap_or(0.0) > 0.0 => {
                let tg = recipe.total_grams.unwrap();
                if let Some(ings) = ings_by_recipe.get(&recipe.id) {
                    let matched_raw: f64 = ings.iter()
                        .filter(|ing| foods.get(&ing.food_id).is_some_and(|f| selector(f)))
                        .map(|ing| ing.grams)
                        .sum();
                    total += matched_raw * eaten / tg;
                }
            }
            // Plain food.
            _ => {
                if selector(food) {
                    total += eaten;
                }
            }
        }
    }
    total
}

/// EATEN grams of vegetable/fruit on `date` (recipe-composition aware).
pub async fn veg_fruit_grams_on(date: &str) -> f64 {
    food_tag_grams_on(date, is_veg_fruit_food).await
}

/// EATEN grams of eggs on `date` (recipe-composition aware): counts eggs used as
/// recipe ingredients too (e.g. the eggs inside a slice of cake).
pub async fn egg_grams_on(date: &str) -> f64 {
    food_tag_grams_on(date, is_egg_food).await
}

/// EATEN grams of red / processed meat on `date` (recipe-composition aware).
pub async fn red_meat_grams_on(date: &str) -> f64 {
    food_tag_grams_on(date, is_red_meat_food).await
}

/// Total amount of the custom nutrient `key` eaten on `date` — sum of
/// `food.nutrients[key] * eaten_grams / 100` over the day's entries. Recipe dishes
/// already carry the SUMMED ingredient nutrients per 100 g (see `finish_recipe`),
/// so no composition expansion is needed here. Unit is whatever the nutrient uses.
pub async fn nutrient_grams_on(date: &str, key: &str) -> f64 {
    let entries = list_diary(date).await;
    let foods = foods_by_ids(entries.iter().map(|e| e.food_id.clone())).await;
    entries.iter().filter_map(|e| {
        foods.get(&e.food_id).and_then(|f| f.nutrients.get(key)).map(|v| {
            let eaten = (e.grams - e.waste_grams).max(0.0);
            v * eaten / 100.0
        })
    }).sum()
}

/// Total protein (grams) eaten on `date` — sum of `protein * eaten_grams / 100`
/// over the day's diary entries (honouring waste).
pub async fn protein_grams_on(date: &str) -> f64 {
    let entries = list_diary(date).await;
    let foods = foods_by_ids(entries.iter().map(|e| e.food_id.clone())).await;
    entries.iter().filter_map(|e| {
        foods.get(&e.food_id).map(|f| {
            let eaten = (e.grams - e.waste_grams).max(0.0);
            f.protein * eaten / 100.0
        })
    }).sum()
}

/// Logical "yesterday" (today - 1 day) as "YYYY-MM-DD", pivoting on the 04:00
/// day boundary like [`today`].
pub fn yesterday() -> String {
    (today_date() - chrono::Duration::days(1))
        .format("%Y-%m-%d")
        .to_string()
}

/// Resolve a food whose `is_restaurant` flag matches `want`. If `food` already
/// matches it's stored and returned as-is. Otherwise we Copy-on-Write: reuse an
/// existing identical variant carrying the wanted flag, or create a fresh Food —
/// leaving the original untouched so other (e.g. past) diary entries keep it.
async fn food_with_restaurant_flag(food: &Food, want: bool) -> Food {
    if food.is_restaurant == want {
        db::put("foods", food).await;
        return food.clone();
    }
    let existing = list_foods().await.into_iter().find(|f| {
        f.id != food.id
            && f.is_restaurant == want
            && f.name == food.name
            && f.is_recipe == food.is_recipe
            && f.recipe_id == food.recipe_id
            && f.kcal == food.kcal
            && f.protein == food.protein
            && f.fat == food.fat
            && f.carbs == food.carbs
            && f.package_weight == food.package_weight
            && f.nutrients == food.nutrients
    });
    if let Some(variant) = existing {
        return variant;
    }
    let variant = Food {
        id: new_id(),
        is_restaurant: want,
        created_at: now(),
        updated_at: now(),
        ..food.clone()
    };
    db::put("foods", &variant).await;
    variant
}

pub async fn save_food_to_diary(
    food: &Food,
    grams: f64,
    waste_grams: f64,
    is_restaurant: bool,
) -> DiaryEntry {
    let food = food_with_restaurant_flag(food, is_restaurant).await;
    let entry = DiaryEntry {
        id: new_id(),
        food_id: food.id.clone(),
        date: today(),
        time: Some(time_now()),
        grams,
        waste_grams,
        meal_label: None,
        deleted: false,
        created_at: now(),
        updated_at: now(),
    };
    db::put("diary", &entry).await;
    // Adding any food to the diary fires the "first food entries" story task.
    crate::services::story::fire_first_food_if_armed().await;
    // Adding a cooked dish (recipe) to the diary — the "I cook" task 2 milestone.
    if food.is_recipe {
        crate::services::story::set_flag(crate::services::story::COOKING_DISH_IN_DIARY, true).await;
    }
    // Recording inedible waste — the "food with bones" task milestone.
    if waste_grams > 0.0 {
        crate::services::story::set_flag(crate::services::story::BONES_WASTE_ENTERED, true).await;
    }
    // Logging restaurant food — the "party or restaurant" task milestone.
    if is_restaurant {
        crate::services::story::set_flag(crate::services::story::RESTAURANT_FOOD_ENTERED, true).await;
    }
    // Classify this food's categories in the background (snack / liquid calories /
    // vegetable-fruit) as soon as it's logged.
    crate::services::classify::enqueue(food.id.clone());
    entry
}

pub async fn update_diary_entry(
    id: &str,
    grams: f64,
    waste_grams: f64,
    is_restaurant: bool,
) -> Option<DiaryEntry> {
    let mut entry: DiaryEntry = db::get("diary", id).await?;
    let food: Food = db::get("foods", &entry.food_id).await?;
    let food = food_with_restaurant_flag(&food, is_restaurant).await;
    entry.food_id = food.id.clone();
    entry.grams = grams;
    entry.waste_grams = waste_grams;
    entry.updated_at = now();
    db::put("diary", &entry).await;
    // The day's aggregates changed → drop its cached indicator values.
    crate::services::indicators::invalidate_day(&entry.date).await;
    if waste_grams > 0.0 {
        crate::services::story::set_flag(crate::services::story::BONES_WASTE_ENTERED, true).await;
    }
    if is_restaurant {
        crate::services::story::set_flag(crate::services::story::RESTAURANT_FOOD_ENTERED, true).await;
    }
    // Editing may fork a new food variant (restaurant CoW) — classify it.
    crate::services::classify::enqueue(entry.food_id.clone());
    Some(entry)
}

/// Stores whose rows can be deleted via the deletion log (kind == store name).
const DELETABLE_STORES: &[&str] = &[
    "foods", "diary", "recipes", "recipe_ingredients", "goals", "weight_entries", "step_entries",
];

/// Record an explicit deletion (tombstone) for `target_id` in store `kind`. The
/// record is synced and re-applied on every device; the local row is removed by
/// the caller (and re-removed by [`apply_deletions`] after each pull, since the
/// server never hard-deletes the entity — it only accumulates these records).
pub async fn record_deletion(kind: &str, target_id: &str) {
    let rec = api_types::DeletionRecord {
        id: new_id(),
        kind: kind.to_string(),
        target_id: target_id.to_string(),
        created_at: now(),
    };
    db::put("deletions", &rec).await;
}

/// Apply every known deletion record: remove the target row from its store. Run
/// after each pull so deletions made on other devices take effect locally.
pub async fn apply_deletions() {
    let dels: Vec<api_types::DeletionRecord> = db::list_all("deletions").await;
    for d in dels {
        if DELETABLE_STORES.contains(&d.kind.as_str()) {
            db::delete(&d.kind, &d.target_id).await;
        }
    }
}

pub async fn remove_food_diary(entry_id: &str) -> Result<(), String> {
    let entry: DiaryEntry = db::get("diary", entry_id)
        .await
        .ok_or_else(|| "entry not found".to_string())?;
    if entry.date != today() {
        return Err("can only delete today's entries".to_string());
    }
    record_deletion("diary", entry_id).await;
    db::delete("diary", entry_id).await;
    crate::services::indicators::invalidate_day(&entry.date).await;
    Ok(())
}

/// Danger zone — reset story progress. Sets every flag to `false` with a fresh
/// `updated_at` (rather than hard-clearing) so the reset is a last-writer-wins
/// update that PROPAGATES across devices via sync — a plain `db::clear` would be
/// re-populated from the server on the next pull. Caller pushes afterwards.
pub async fn delete_story_progress() {
    let flags: Vec<api_types::StoryFlag> = db::list_all("story").await;
    let ts = now();
    for mut f in flags {
        f.value = false;
        f.updated_at = ts.clone();
        db::put("story", &f).await;
    }
    // Also drop the attached progress photos (a local-only, un-synced store):
    // resetting the story must clear the "before" photos too.
    db::clear("progress_photos").await;
}

/// Danger zone — delete diary food entries (and their cached day-summaries).
/// `cutoff` (YYYY-MM-DD): entries with `date < cutoff` are removed; `None`
/// removes ALL. Uses a SOFT delete (`deleted=true`, bumped `updated_at`) — that's
/// the ONLY form the server honours (`/sync/push` upserts and only tombstones via
/// `deleted`; absent rows are left intact), so the caller must `sync::push_*`
/// afterwards for it to propagate. Day/week summaries are a local-only derived
/// cache, so they're hard-deleted (keeping the `meta:` markers).
pub async fn delete_diary_data(cutoff: Option<&str>) {
    let entries: Vec<DiaryEntry> = db::list_all("diary").await;
    for e in entries {
        if e.deleted {
            continue;
        }
        if cutoff.map_or(true, |c| e.date.as_str() < c) {
            record_deletion("diary", &e.id).await;
            db::delete("diary", &e.id).await;
        }
    }
    let summaries: Vec<crate::services::summary::Summary> = db::list_all("summaries").await;
    for s in summaries {
        if s.id.starts_with("meta:") {
            continue;
        }
        if cutoff.map_or(true, |c| s.date.as_str() < c) {
            db::delete("summaries", &s.id).await;
        }
    }
    // A bulk delete can touch arbitrary past days → drop the whole indicator cache.
    crate::services::indicators::clear_cache().await;
}

/// Duplicate a diary entry as a NEW entry today with a fresh time (food and
/// grams/waste copied). Used by the diary-row long-press "Duplicate" action.
pub async fn duplicate_diary_entry(entry_id: &str) -> Option<DiaryEntry> {
    let src: DiaryEntry = db::get("diary", entry_id).await?;
    let entry = DiaryEntry {
        id: new_id(),
        food_id: src.food_id.clone(),
        date: today(),
        time: Some(time_now()),
        grams: src.grams,
        waste_grams: src.waste_grams,
        meal_label: src.meal_label.clone(),
        deleted: false,
        created_at: now(),
        updated_at: now(),
    };
    db::put("diary", &entry).await;
    crate::services::classify::enqueue(entry.food_id.clone());
    Some(entry)
}

/// Edit the product (name + KBJU + custom nutrients) behind a diary entry.
/// Copy-on-write: if the product is referenced ONLY by this entry, edit it in
/// place; if it's shared (any other non-deleted diary entry, or any recipe
/// ingredient), create a copy with the edits and repoint just this entry — so
/// other usages keep the original values.
pub async fn edit_food_for_entry(
    entry_id: &str,
    name: String,
    kcal: f64,
    protein: f64,
    fat: f64,
    carbs: f64,
    nutrients: BTreeMap<String, f64>,
) -> Option<()> {
    let mut entry: DiaryEntry = db::get("diary", entry_id).await?;
    let food: Food = db::get("foods", &entry.food_id).await?;

    // Our category tags AND the enriched nutrients (calcium/iron/omega-3/fiber) are
    // bound to the product NAME. When the name changes they're stale, so clear them
    // and let the background queue re-classify + re-enrich under the new name. When
    // the name is unchanged we keep whatever was already computed.
    let renamed = name != food.name;
    let mut nutrients = nutrients;
    let (is_snack, is_liquid_cal, is_veg_fruit, is_egg, is_red_meat) = if renamed {
        for key in crate::services::enrich::nutrient_names() {
            nutrients.remove(key);
        }
        (None, None, None, None, None)
    } else {
        (food.is_snack, food.is_liquid_cal, food.is_veg_fruit, food.is_egg, food.is_red_meat)
    };

    let all_diary: Vec<DiaryEntry> = db::list_all("diary").await;
    let other_diary = all_diary
        .iter()
        .any(|e| e.food_id == food.id && e.id != entry.id && !e.deleted);
    let recipe_refs: Vec<RecipeIngredient> =
        db::list_by_index("recipe_ingredients", "food_id", &food.id).await;
    let shared = other_diary || !recipe_refs.is_empty();

    if shared {
        let copy = Food {
            id: new_id(),
            name, kcal, protein, fat, carbs, nutrients,
            is_snack, is_liquid_cal, is_veg_fruit, is_egg, is_red_meat,
            created_at: now(),
            updated_at: now(),
            ..food.clone()
        };
        db::put("foods", &copy).await;
        entry.food_id = copy.id.clone();
        entry.updated_at = now();
        db::put("diary", &entry).await;
        crate::services::classify::enqueue(copy.id);
    } else {
        let updated = Food {
            name, kcal, protein, fat, carbs, nutrients,
            is_snack, is_liquid_cal, is_veg_fruit, is_egg, is_red_meat,
            updated_at: now(),
            ..food.clone()
        };
        db::put("foods", &updated).await;
        crate::services::classify::enqueue(updated.id);
    }
    Some(())
}

// --- Food Drafts ---

pub async fn save_draft(food: &Food) -> FoodDraft {
    let new_keys: std::collections::BTreeSet<&str> =
        food.nutrients.keys().map(|k| k.as_str()).collect();

    let existing: Vec<FoodDraft> = db::list_all("food_drafts").await;
    let matched = existing.into_iter().find(|d| {
        d.name == food.name
            && d.nutrients.len() == new_keys.len()
            && d.nutrients.keys().all(|k| new_keys.contains(k.as_str()))
    });

    if let Some(mut draft) = matched {
        draft.kcal = food.kcal;
        draft.protein = food.protein;
        draft.fat = food.fat;
        draft.carbs = food.carbs;
        draft.nutrients = food.nutrients.clone();
        draft.package_weight = food.package_weight;
        draft.created_at = now();
        db::put("food_drafts", &draft).await;
        return draft;
    }

    let draft = FoodDraft {
        id: new_id(),
        name: food.name.clone(),
        kcal: food.kcal,
        protein: food.protein,
        fat: food.fat,
        carbs: food.carbs,
        nutrients: food.nutrients.clone(),
        package_weight: food.package_weight,
        food_id: None,
        created_at: now(),
    };
    db::put("food_drafts", &draft).await;
    draft
}

pub async fn update_draft_fields(draft_id: &str, food: &Food) {
    if let Some(mut draft) = db::get::<FoodDraft>("food_drafts", draft_id).await {
        draft.name = food.name.clone();
        draft.kcal = food.kcal;
        draft.protein = food.protein;
        draft.fat = food.fat;
        draft.carbs = food.carbs;
        draft.nutrients = food.nutrients.clone();
        draft.package_weight = food.package_weight;
        let linked_food_id = draft.food_id.clone();
        db::put("food_drafts", &draft).await;

        // Keep the Food created from this draft in sync — editing the name (or
        // КБЖУ) of a recognized product must update both the draft AND its Food.
        // Only the editable fields change; id / flags / timestamps are preserved.
        if let Some(fid) = linked_food_id {
            if let Some(mut f) = db::get::<Food>("foods", &fid).await {
                f.name = food.name.clone();
                f.kcal = food.kcal;
                f.protein = food.protein;
                f.fat = food.fat;
                f.carbs = food.carbs;
                f.nutrients = food.nutrients.clone();
                f.package_weight = food.package_weight;
                f.updated_at = now();
                db::put("foods", &f).await;
            }
        }
    }
}

pub async fn set_draft_food_id(draft_id: &str, food_id: &str) {
    if let Some(mut draft) = db::get::<FoodDraft>("food_drafts", draft_id).await {
        draft.food_id = Some(food_id.to_string());
        db::put("food_drafts", &draft).await;
    }
}

pub async fn list_drafts() -> Vec<FoodDraft> {
    let mut drafts: Vec<FoodDraft> = db::list_all("food_drafts").await;
    drafts.sort_by(|a, b| b.created_at.cmp(&a.created_at));
    drafts
}

/// Add a draft to diary: create Food if not yet created, then diary entry.
pub async fn add_draft_to_diary(draft_id: &str, grams: f64) -> Option<DiaryEntry> {
    let mut draft: FoodDraft = db::get("food_drafts", draft_id).await?;

    let food_id = if let Some(ref fid) = draft.food_id {
        fid.clone()
    } else {
        let food = Food {
            id: new_id(),
            name: draft.name.clone(),
            kcal: draft.kcal,
            protein: draft.protein,
            fat: draft.fat,
            carbs: draft.carbs,
            nutrients: draft.nutrients.clone(),
            package_weight: draft.package_weight,
            is_recipe: false,
            recipe_id: None,
            archived: false,
            is_restaurant: false,
            is_snack: None,
            is_liquid_cal: None,
            is_veg_fruit: None, is_egg: None, is_red_meat: None,
            created_at: now(),
            updated_at: now(),
        };
        db::put("foods", &food).await;
        draft.food_id = Some(food.id.clone());
        db::put("food_drafts", &draft).await;
        food.id
    };

    let food: Food = db::get("foods", &food_id).await?;
    let entry = DiaryEntry {
        id: new_id(),
        food_id: food.id,
        date: today(),
        time: Some(time_now()),
        grams,
        waste_grams: 0.0,
        meal_label: None,
        deleted: false,
        created_at: now(),
        updated_at: now(),
    };
    db::put("diary", &entry).await;
    // Entering a food into the diary fires the "first food entries" story task
    // (only counts if its trigger was armed by opening that section).
    crate::services::story::fire_first_food_if_armed().await;
    Some(entry)
}

// --- Recipes ---

pub async fn list_recipes() -> Vec<Recipe> {
    db::list_all("recipes").await
}

/// All recipe ingredients across every recipe (id, recipe_id, food_id, grams).
pub async fn list_recipe_ingredients() -> Vec<RecipeIngredient> {
    db::list_all("recipe_ingredients").await
}

pub async fn new_recipe(name: &str) -> Recipe {
    let recipe = Recipe {
        id: new_id(),
        name: name.to_string(),
        notes: None,
        total_grams: None,
        finalized: false,
        food_id: None,
        superseded_by: None,
        ingredients: Vec::new(),
        created_at: now(),
        updated_at: now(),
    };
    db::put("recipes", &recipe).await;
    recipe
}

pub async fn get_recipe(id: &str) -> Option<Recipe> {
    let mut recipe: Recipe = db::get("recipes", id).await?;
    let ingredients: Vec<RecipeIngredient> =
        db::list_by_index("recipe_ingredients", "recipe_id", id).await;
    recipe.ingredients = ingredients;
    Some(recipe)
}

pub async fn change_recipe_name(id: &str, name: &str) -> Option<Recipe> {
    let mut recipe: Recipe = db::get("recipes", id).await?;
    recipe.name = name.to_string();
    recipe.updated_at = now();
    db::put("recipes", &recipe).await;
    get_recipe(id).await
}

pub async fn clone_recipe(id: &str) -> Option<Recipe> {
    let source = get_recipe(id).await?;
    let new_id = new_id();
    let ts = now();

    let mut cloned = Recipe {
        id: new_id.clone(),
        name: source.name.clone(),
        notes: source.notes.clone(),
        total_grams: None,
        finalized: false,
        food_id: None,
        superseded_by: None,
        ingredients: Vec::new(),
        created_at: ts.clone(),
        updated_at: ts.clone(),
    };
    db::put("recipes", &cloned).await;

    for ing in &source.ingredients {
        let new_ing = RecipeIngredient {
            id: self::new_id(),
            recipe_id: new_id.clone(),
            food_id: ing.food_id.clone(),
            grams: ing.grams,
            created_at: ts.clone(),
            updated_at: ts.clone(),
        };
        db::put("recipe_ingredients", &new_ing).await;
        cloned.ingredients.push(new_ing);
    }

    // Cooking again: mark the PARENT recipe as superseded by this new one. Explicit
    // parentage (id of the successor), never inferred from the mutable name. The
    // parent then drops out of the recipes list and the diary food search; its
    // finished food stays referenced by past diary entries. (The stored "recipes"
    // row keeps ingredients separately, so re-put here doesn't duplicate them.)
    if let Some(mut parent) = db::get::<Recipe>("recipes", id).await {
        parent.superseded_by = Some(new_id.clone());
        parent.updated_at = now();
        db::put("recipes", &parent).await;
    }

    Some(cloned)
}

// --- Recipe Ingredients ---

pub async fn add_ingredient_to_recipe(
    food: &Food,
    grams: f64,
    recipe_id: &str,
) -> RecipeIngredient {
    db::put("foods", food).await;
    // Classify the ingredient itself (not just the finished dish), so a recipe's
    // egg / red-meat / veg-fruit content can be counted by composition later.
    crate::services::classify::enqueue(food.id.clone());
    let ing = RecipeIngredient {
        id: new_id(),
        recipe_id: recipe_id.to_string(),
        food_id: food.id.clone(),
        grams,
        created_at: now(),
        updated_at: now(),
    };
    db::put("recipe_ingredients", &ing).await;
    ing
}

pub async fn update_ingredient(id: &str, grams: f64) -> Option<RecipeIngredient> {
    let mut ing: RecipeIngredient = db::get("recipe_ingredients", id).await?;
    ing.grams = grams;
    ing.updated_at = now();
    db::put("recipe_ingredients", &ing).await;
    Some(ing)
}

pub async fn remove_ingredient(id: &str) {
    record_deletion("recipe_ingredients", id).await;
    db::delete("recipe_ingredients", id).await;
}

// --- Finalize Recipe ---

pub async fn finish_recipe(recipe_id: &str, total_grams: f64) -> Option<Food> {
    let recipe = get_recipe(recipe_id).await?;
    let foods = list_foods().await;

    let mut total_kcal = 0.0_f64;
    let mut total_protein = 0.0_f64;
    let mut total_fat = 0.0_f64;
    let mut total_carbs = 0.0_f64;
    let mut total_nutrients = BTreeMap::<String, f64>::new();

    for ing in &recipe.ingredients {
        if let Some(f) = foods.iter().find(|f| f.id == ing.food_id) {
            let factor = ing.grams / 100.0;
            total_kcal += f.kcal * factor;
            total_protein += f.protein * factor;
            total_fat += f.fat * factor;
            total_carbs += f.carbs * factor;
            for (k, v) in &f.nutrients {
                *total_nutrients.entry(k.clone()).or_default() += v * factor;
            }
        }
    }

    let scale = 100.0 / total_grams;
    let nutrients = total_nutrients
        .into_iter()
        .map(|(k, v)| (k, v * scale))
        .collect();

    let food = Food {
        id: new_id(),
        name: recipe.name.clone(),
        kcal: total_kcal * scale,
        protein: total_protein * scale,
        fat: total_fat * scale,
        carbs: total_carbs * scale,
        nutrients,
        package_weight: None,
        is_recipe: true,
        recipe_id: Some(recipe_id.to_string()),
        archived: false,
        is_restaurant: false,
        is_snack: None,
        is_liquid_cal: None,
        is_veg_fruit: None,
        is_egg: None,
        is_red_meat: None,
        created_at: now(),
        updated_at: now(),
    };
    db::put("foods", &food).await;

    let mut updated_recipe: Recipe = db::get("recipes", recipe_id).await?;
    updated_recipe.finalized = true;
    updated_recipe.total_grams = Some(total_grams);
    updated_recipe.food_id = Some(food.id.clone());
    updated_recipe.updated_at = now();
    db::put("recipes", &updated_recipe).await;

    // NB: the previous version of a re-cooked dish is hidden purely at DISPLAY time
    // (newest-recipe-per-name) in both the recipes list (`recipes.rs`) and the food
    // picker (`food_picker.rs`) — no `archived` write here, so pre-existing
    // duplicates are handled too and there's no data migration.

    // Finishing a recipe creates a dish — the "I cook" task 1 milestone.
    crate::services::story::set_flag(crate::services::story::COOKING_DISH_CREATED, true).await;

    // Ensure every ingredient is classified, so the dish's egg / red-meat / veg-fruit
    // content can be counted by composition when it's logged.
    for ing in &recipe.ingredients {
        crate::services::classify::enqueue(ing.food_id.clone());
    }

    Some(food)
}

/// Change a finalized recipe's final total weight and REPRICE its finished food
/// in place. The dish's total nutrients (summed from ingredients) are fixed; only
/// the per-100g density changes, so recompute `100/new_total_grams` and update the
/// SAME food row (recipe.food_id) plus the recipe's `total_grams`. Any diary entry
/// referencing this food picks up the new per-100g values automatically.
pub async fn change_recipe_weight(recipe_id: &str, new_total_grams: f64) -> Option<Food> {
    if new_total_grams <= 0.0 {
        return None;
    }
    let recipe = get_recipe(recipe_id).await?;
    let food_id = recipe.food_id.clone()?;
    let foods = list_foods().await;

    let mut total_kcal = 0.0_f64;
    let mut total_protein = 0.0_f64;
    let mut total_fat = 0.0_f64;
    let mut total_carbs = 0.0_f64;
    let mut total_nutrients = BTreeMap::<String, f64>::new();
    for ing in &recipe.ingredients {
        if let Some(f) = foods.iter().find(|f| f.id == ing.food_id) {
            let factor = ing.grams / 100.0;
            total_kcal += f.kcal * factor;
            total_protein += f.protein * factor;
            total_fat += f.fat * factor;
            total_carbs += f.carbs * factor;
            for (k, v) in &f.nutrients {
                *total_nutrients.entry(k.clone()).or_default() += v * factor;
            }
        }
    }

    let scale = 100.0 / new_total_grams;
    let mut food: Food = db::get("foods", &food_id).await?;
    food.kcal = total_kcal * scale;
    food.protein = total_protein * scale;
    food.fat = total_fat * scale;
    food.carbs = total_carbs * scale;
    food.nutrients = total_nutrients.into_iter().map(|(k, v)| (k, v * scale)).collect();
    food.updated_at = now();
    db::put("foods", &food).await;

    let mut updated_recipe: Recipe = db::get("recipes", recipe_id).await?;
    updated_recipe.total_grams = Some(new_total_grams);
    updated_recipe.updated_at = now();
    db::put("recipes", &updated_recipe).await;

    Some(food)
}

// --- Goals ---

pub async fn list_goals() -> Vec<Goal> {
    db::list_all("goals").await
}

pub async fn create_goal(input: CreateGoalInput) -> Goal {
    let key = api_types::nutrient_key::generate(&input.nutrient);
    let goal = Goal {
        id: new_id(),
        nutrient: input.nutrient,
        key,
        direction: input.direction,
        amount: input.amount,
        unit: input.unit,
        period: input.period,
        created_at: now(),
        updated_at: now(),
    };
    db::put("goals", &goal).await;
    goal
}

pub async fn update_goal(goal: &Goal) {
    db::put("goals", goal).await;
}

pub async fn delete_goal(id: &str) {
    record_deletion("goals", id).await;
    db::delete("goals", id).await;
}

/// Create or update the hidden daily-Calories `AtMost` goal (the "planka") to
/// `amount` kcal. Used when the user accepts the calorie planka in ch3.
pub async fn set_calorie_goal(amount: f64) {
    match list_goals().await.into_iter().find(|g| g.nutrient == "Calories") {
        Some(mut g) => {
            g.direction = GoalDirection::AtMost;
            g.amount = amount;
            g.unit = GoalUnit::Kcal;
            g.period = GoalPeriod::Day;
            g.updated_at = now();
            update_goal(&g).await;
        }
        None => {
            create_goal(CreateGoalInput {
                nutrient: "Calories".to_string(),
                direction: GoalDirection::AtMost,
                amount,
                unit: GoalUnit::Kcal,
                period: GoalPeriod::Day,
            })
            .await;
        }
    }
    // The planka now matches the current goal/trend again.
    set_planka_stale(false);
}

// ── "Planka needs recalculating" signal ──────────────────────────────────────
// Raised when the course goal changes (the old planka no longer fits the new
// goal); cleared whenever the planka is (re)computed. Persisted per device.
const PLANKA_STALE_KEY: &str = "planka_stale";
thread_local! {
    static PLANKA_STALE: std::cell::RefCell<Option<leptos::RwSignal<bool>>> =
        const { std::cell::RefCell::new(None) };
}

/// Create the signal at the root, seeded from the persisted flag. Call from main().
pub fn init_planka_stale() {
    PLANKA_STALE.with(|c| {
        if c.borrow().is_none() {
            *c.borrow_mut() = Some(leptos::create_rw_signal(
                crate::services::app_flags::get_bool(PLANKA_STALE_KEY),
            ));
        }
    });
}

/// Reactive flag: the planka should be recomputed (the course goal changed).
pub fn planka_stale_signal() -> leptos::RwSignal<bool> {
    PLANKA_STALE.with(|c| c.borrow().expect("init_planka_stale() must run first"))
}

/// Set/clear the "planka needs recalculating" flag (persisted + reactive).
pub fn set_planka_stale(v: bool) {
    use leptos::SignalSet;
    crate::services::app_flags::set_bool(PLANKA_STALE_KEY, v);
    planka_stale_signal().set(v);
}

/// Create or update the daily Steps `AtLeast` goal to `amount` steps. A real
/// persisted goal (same machinery as nutrient goals), set when the user accepts
/// the weekly card's steps lever; backs the steps_planka source threshold.
pub async fn set_steps_goal(amount: f64) {
    match list_goals().await.into_iter().find(|g| g.nutrient == "Steps") {
        Some(mut g) => {
            g.direction = GoalDirection::AtLeast;
            g.amount = amount;
            g.unit = GoalUnit::Steps;
            g.period = GoalPeriod::Day;
            g.updated_at = now();
            update_goal(&g).await;
        }
        None => {
            create_goal(CreateGoalInput {
                nutrient: "Steps".to_string(),
                direction: GoalDirection::AtLeast,
                amount,
                unit: GoalUnit::Steps,
                period: GoalPeriod::Day,
            })
            .await;
        }
    }
}

/// The current daily steps target from the Steps `AtLeast` goal, if set.
pub async fn steps_goal_amount() -> Option<f64> {
    list_goals()
        .await
        .into_iter()
        .find(|g| g.nutrient == "Steps" && g.direction == GoalDirection::AtLeast && g.amount > 0.0)
        .map(|g| g.amount)
}

// --- Weight Entries ---

pub async fn save_weight(weight_kg: f64, no_water: bool, no_food: bool, no_wash: bool, used_toilet: bool, morning: bool) -> WeightEntry {
    let date = today();
    if let Some(mut existing) = get_weight_for_date(&date).await {
        existing.weight_kg = weight_kg;
        existing.no_water = no_water;
        existing.no_food = no_food;
        existing.no_wash = no_wash;
        existing.used_toilet = used_toilet;
        existing.morning = morning;
        existing.updated_at = now();
        db::put("weight_entries", &existing).await;
        return existing;
    }
    let entry = WeightEntry {
        id: new_id(),
        date,
        weight_kg,
        no_water,
        no_food,
        no_wash,
        used_toilet,
        morning,
        created_at: now(),
        updated_at: now(),
    };
    db::put("weight_entries", &entry).await;
    entry
}

pub async fn get_weight_for_date(date: &str) -> Option<WeightEntry> {
    let entries: Vec<WeightEntry> = db::list_by_index("weight_entries", "date", date).await;
    entries.into_iter().next()
}

pub async fn list_weight_entries() -> Vec<WeightEntry> {
    let mut entries: Vec<WeightEntry> = db::list_all("weight_entries").await;
    entries.sort_by(|a, b| a.date.cmp(&b.date));
    entries
}

// --- Step Entries ---

pub async fn save_steps(date: &str, steps: u32) -> api_types::StepEntry {
    let entry = if let Some(mut existing) = get_steps_for_date(date).await {
        existing.steps = steps;
        existing.updated_at = now();
        db::put("step_entries", &existing).await;
        existing
    } else {
        let entry = api_types::StepEntry {
            id: new_id(),
            date: date.to_string(),
            steps,
            created_at: now(),
            updated_at: now(),
        };
        db::put("step_entries", &entry).await;
        entry
    };
    // Recording steps is the milestone event for the activity section's task 2.
    crate::services::story::set_flag(crate::services::story::FIRST_STEPS, true).await;
    entry
}

pub async fn get_steps_for_date(date: &str) -> Option<api_types::StepEntry> {
    let entries: Vec<api_types::StepEntry> = db::list_by_index("step_entries", "date", date).await;
    entries.into_iter().next()
}

pub async fn list_step_entries() -> Vec<api_types::StepEntry> {
    let mut entries: Vec<api_types::StepEntry> = db::list_all("step_entries").await;
    entries.sort_by(|a, b| a.date.cmp(&b.date));
    entries
}

// --- Progress photos (client-only: front / side / back, for tracking) ---

/// One of the three required poses.
pub const POSES: [&str; 3] = ["front", "side", "back"];

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ProgressPhoto {
    pub id: String,
    pub pose: String,
    pub date: String,
    /// Image as a data URL (base64).
    pub image: String,
    pub created_at: String,
}

pub async fn save_progress_photo(pose: &str, image: &str) -> ProgressPhoto {
    let photo = ProgressPhoto {
        id: new_id(),
        pose: pose.to_string(),
        date: today(),
        image: image.to_string(),
        created_at: now(),
    };
    db::put("progress_photos", &photo).await;
    // The intro task completes once all three poses have at least one photo.
    let all: Vec<ProgressPhoto> = db::list_all("progress_photos").await;
    let have: std::collections::BTreeSet<&str> = all.iter().map(|p| p.pose.as_str()).collect();
    if POSES.iter().all(|p| have.contains(p)) {
        crate::services::story::set_flag(crate::services::story::PROGRESS_PHOTOS_TAKEN, true).await;
    }
    photo
}

pub async fn list_progress_photos() -> Vec<ProgressPhoto> {
    let mut photos: Vec<ProgressPhoto> = db::list_all("progress_photos").await;
    photos.sort_by(|a, b| b.created_at.cmp(&a.created_at)); // newest first
    photos
}

#[cfg(test)]
mod tests {
    use super::calorie_planka;
    use crate::services::weight_trend::BalanceState;

    #[test]
    fn calorie_planka_deficit_is_avg_rounded_to_50() {
        // Deficit -> avg itself, rounded to nearest 50.
        assert_eq!(calorie_planka(2000.0, BalanceState::Deficit), 2000.0);
        assert_eq!(calorie_planka(2490.0, BalanceState::Deficit), 2500.0); // 49.8 -> 50
        assert_eq!(calorie_planka(2470.0, BalanceState::Deficit), 2450.0); // 49.4 -> 49
        assert_eq!(calorie_planka(2475.0, BalanceState::Deficit), 2500.0); // 49.5 -> 50
    }

    #[test]
    fn calorie_planka_non_deficit_is_minus_5pct_rounded_to_50() {
        // Maintenance / Surplus -> avg * 0.95, rounded to nearest 50.
        // 2000 * 0.95 = 1900.0
        assert_eq!(calorie_planka(2000.0, BalanceState::Maintenance), 1900.0);
        assert_eq!(calorie_planka(2000.0, BalanceState::Surplus), 1900.0);
        // 2100 * 0.95 = 1995.0 -> rounds to 2000.
        assert_eq!(calorie_planka(2100.0, BalanceState::Maintenance), 2000.0);
        // 2050 * 0.95 = 1947.5 -> 38.95 -> 39 -> 1950.
        assert_eq!(calorie_planka(2050.0, BalanceState::Surplus), 1950.0);
    }
}

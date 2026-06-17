use std::collections::BTreeMap;

use api_types::*;

use super::db;

fn now() -> String {
    chrono::Utc::now().to_rfc3339()
}

fn new_id() -> String {
    uuid::Uuid::now_v7().to_string()
}

fn today() -> String {
    chrono::Local::now().format("%Y-%m-%d").to_string()
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

// --- Chapter 2 detection helpers ---
//
// Case-insensitive substring name-matching on `food.name.to_lowercase()`. These
// back the chapter-2 section tasks and the daily report's per-day facts.

/// Substrings that mark a low-calorie SNACK food (chapter 2 / s4).
const SNACK_SUBSTRINGS: &[&str] = &[
    "огурец", "помидор", "томат", "попкорн", "яблок", "морков", "редис",
    "сельдере", "капуст", "болгарск",
];

/// Substrings that mark a DRINK food (chapter 2 / s5).
const DRINK_SUBSTRINGS: &[&str] = &[
    "сок", "газиров", "кола", "лимонад", "морс", "квас", "компот", "нектар",
    "энергетик", "пепси", "фанта", "спрайт", "cola", "pepsi", "sprite", "fanta",
];

fn name_matches(name: &str, needles: &[&str]) -> bool {
    let lower = name.to_lowercase();
    needles.iter().any(|n| lower.contains(n))
}

/// True if `food` is a low-calorie snack by name.
pub fn is_snack_food(food: &Food) -> bool {
    name_matches(&food.name, SNACK_SUBSTRINGS)
}

/// True if `food` is a drink by name.
pub fn is_drink_food(food: &Food) -> bool {
    name_matches(&food.name, DRINK_SUBSTRINGS)
}

/// A drink is HIGH-CAL if its per-100g kcal > 30.
pub fn is_high_cal_drink(food: &Food) -> bool {
    is_drink_food(food) && food.kcal > 30.0
}

/// A drink is ZERO-CAL if its per-100g kcal <= 5.
pub fn is_zero_cal_drink(food: &Food) -> bool {
    is_drink_food(food) && food.kcal <= 5.0
}

/// Build a food-id → Food lookup over all stored foods.
async fn food_map() -> BTreeMap<String, Food> {
    list_foods().await.into_iter().map(|f| (f.id.clone(), f)).collect()
}

/// True if the diary for `date` contains at least one SNACK food.
pub async fn snack_logged_on(date: &str) -> bool {
    let foods = food_map().await;
    list_diary(date).await.iter().any(|e| {
        foods.get(&e.food_id).map_or(false, is_snack_food)
    })
}

/// True if the diary for `date` contains at least one HIGH-CAL drink.
pub async fn high_cal_drink_on(date: &str) -> bool {
    let foods = food_map().await;
    list_diary(date).await.iter().any(|e| {
        foods.get(&e.food_id).map_or(false, is_high_cal_drink)
    })
}

/// True if the diary for `date` contains at least one ZERO-CAL drink.
pub async fn zero_cal_drink_on(date: &str) -> bool {
    let foods = food_map().await;
    list_diary(date).await.iter().any(|e| {
        foods.get(&e.food_id).map_or(false, is_zero_cal_drink)
    })
}

/// Evening protein (grams) on `date`: sum of `protein * eaten_grams / 100`
/// (honouring waste, like the diary) over entries whose derived meal bucket is
/// Dinner or NightSnack.
pub async fn evening_protein_on(date: &str) -> f64 {
    let foods = food_map().await;
    let diary = list_diary(date).await;
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

/// True once a daily report has been generated for `date` (a `summaries`
/// record with id `day:<date>` exists).
pub async fn report_ready_on(date: &str) -> bool {
    crate::services::summary::get_day(date).await.is_some()
}

/// Local "yesterday" (today - 1 day) as "YYYY-MM-DD".
pub fn yesterday() -> String {
    (chrono::Local::now().date_naive() - chrono::Duration::days(1))
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
    if waste_grams > 0.0 {
        crate::services::story::set_flag(crate::services::story::BONES_WASTE_ENTERED, true).await;
    }
    if is_restaurant {
        crate::services::story::set_flag(crate::services::story::RESTAURANT_FOOD_ENTERED, true).await;
    }
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
            created_at: now(),
            updated_at: now(),
            ..food.clone()
        };
        db::put("foods", &copy).await;
        entry.food_id = copy.id.clone();
        entry.updated_at = now();
        db::put("diary", &entry).await;
    } else {
        let updated = Food {
            name, kcal, protein, fat, carbs, nutrients,
            updated_at: now(),
            ..food.clone()
        };
        db::put("foods", &updated).await;
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

pub async fn new_recipe(name: &str) -> Recipe {
    let recipe = Recipe {
        id: new_id(),
        name: name.to_string(),
        notes: None,
        total_grams: None,
        finalized: false,
        food_id: None,
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
        notes: source.notes,
        total_grams: None,
        finalized: false,
        food_id: None,
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

    Some(cloned)
}

// --- Recipe Ingredients ---

pub async fn add_ingredient_to_recipe(
    food: &Food,
    grams: f64,
    recipe_id: &str,
) -> RecipeIngredient {
    db::put("foods", food).await;
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

    // Finishing a recipe creates a dish — the "I cook" task 1 milestone.
    crate::services::story::set_flag(crate::services::story::COOKING_DISH_CREATED, true).await;

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

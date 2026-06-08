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

pub async fn save_food_to_diary(food: &Food, grams: f64) -> DiaryEntry {
    db::put("foods", food).await;
    let entry = DiaryEntry {
        id: new_id(),
        food_id: food.id.clone(),
        date: today(),
        time: Some(time_now()),
        grams,
        meal_label: None,
        deleted: false,
        created_at: now(),
        updated_at: now(),
    };
    db::put("diary", &entry).await;
    entry
}

pub async fn update_diary_entry(id: &str, grams: f64) -> Option<DiaryEntry> {
    let mut entry: DiaryEntry = db::get("diary", id).await?;
    entry.grams = grams;
    entry.updated_at = now();
    db::put("diary", &entry).await;
    Some(entry)
}

pub async fn remove_food_diary(entry_id: &str) -> Result<(), String> {
    let entry: DiaryEntry = db::get("diary", entry_id)
        .await
        .ok_or_else(|| "entry not found".to_string())?;
    if entry.date != today() {
        return Err("can only delete today's entries".to_string());
    }
    let mut entry = entry;
    entry.deleted = true;
    entry.updated_at = now();
    db::put("diary", &entry).await;
    Ok(())
}

// --- Food Drafts ---

pub async fn save_draft(food: &Food) -> FoodDraft {
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
        meal_label: None,
        deleted: false,
        created_at: now(),
        updated_at: now(),
    };
    db::put("diary", &entry).await;
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
    db::delete("goals", id).await;
}

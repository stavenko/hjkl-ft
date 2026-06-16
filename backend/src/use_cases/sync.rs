use api_types::*;

use crate::providers::database::Database;

pub fn dump(db: &Database) -> Result<SyncDumpResponse, ApiError> {
    db.with_conn(|conn| {
        let mut stmt = conn
            .prepare("SELECT id, name, kcal, protein, fat, carbs, nutrients_json, package_weight, is_recipe, recipe_id, archived, is_restaurant, created_at, updated_at FROM food WHERE deleted = 0")
            .map_err(|_| ApiError::InternalError)?;
        let foods: Vec<Food> = stmt.query_map([], |row| {
            let nutrients_json: String = row.get(6)?;
            let nutrients: std::collections::BTreeMap<String, f64> =
                serde_json::from_str(&nutrients_json).unwrap_or_default();
            Ok(Food {
                id: row.get(0)?, name: row.get(1)?, kcal: row.get(2)?,
                protein: row.get(3)?, fat: row.get(4)?, carbs: row.get(5)?,
                nutrients, package_weight: row.get(7)?,
                is_recipe: row.get::<_, i32>(8)? != 0, recipe_id: row.get(9)?,
                archived: row.get::<_, i32>(10)? != 0,
                is_restaurant: row.get::<_, i32>(11)? != 0,
                created_at: row.get(12)?, updated_at: row.get(13)?,
            })
        }).map_err(|_| ApiError::InternalError)?
        .collect::<Result<Vec<_>, _>>().map_err(|_| ApiError::InternalError)?;

        let mut stmt = conn
            .prepare("SELECT id, food_id, date, time, grams, meal_label, waste_grams, created_at, updated_at FROM diary WHERE deleted = 0")
            .map_err(|_| ApiError::InternalError)?;
        let diary_entries: Vec<DiaryEntry> = stmt.query_map([], |row| {
            Ok(DiaryEntry {
                id: row.get(0)?, food_id: row.get(1)?, date: row.get(2)?,
                time: row.get(3)?, grams: row.get(4)?, meal_label: row.get(5)?,
                waste_grams: row.get(6)?,
                deleted: false, created_at: row.get(7)?, updated_at: row.get(8)?,
            })
        }).map_err(|_| ApiError::InternalError)?
        .collect::<Result<Vec<_>, _>>().map_err(|_| ApiError::InternalError)?;

        let mut stmt = conn
            .prepare("SELECT id, name, notes, total_grams, finalized, food_id, created_at, updated_at FROM recipe WHERE deleted = 0")
            .map_err(|_| ApiError::InternalError)?;
        let recipes: Vec<Recipe> = stmt.query_map([], |row| {
            Ok(Recipe {
                id: row.get(0)?, name: row.get(1)?, notes: row.get(2)?,
                total_grams: row.get(3)?, finalized: row.get::<_, i32>(4)? != 0,
                food_id: row.get(5)?, ingredients: Vec::new(),
                created_at: row.get(6)?, updated_at: row.get(7)?,
            })
        }).map_err(|_| ApiError::InternalError)?
        .collect::<Result<Vec<_>, _>>().map_err(|_| ApiError::InternalError)?;

        let mut stmt = conn
            .prepare("SELECT id, recipe_id, food_id, grams, created_at, updated_at FROM recipe_ingredient WHERE deleted = 0")
            .map_err(|_| ApiError::InternalError)?;
        let recipe_ingredients: Vec<RecipeIngredient> = stmt.query_map([], |row| {
            Ok(RecipeIngredient {
                id: row.get(0)?, recipe_id: row.get(1)?, food_id: row.get(2)?,
                grams: row.get(3)?, created_at: row.get(4)?, updated_at: row.get(5)?,
            })
        }).map_err(|_| ApiError::InternalError)?
        .collect::<Result<Vec<_>, _>>().map_err(|_| ApiError::InternalError)?;

        let mut stmt = conn
            .prepare("SELECT id, nutrient, key, direction, amount, unit, period, created_at, updated_at FROM goal WHERE deleted = 0")
            .map_err(|_| ApiError::InternalError)?;
        let goals: Vec<Goal> = stmt.query_map([], |row| {
            let direction: String = row.get(3)?;
            let unit: String = row.get(5)?;
            let period: String = row.get(6)?;
            Ok(Goal {
                id: row.get(0)?, nutrient: row.get(1)?, key: row.get(2)?,
                direction: match direction.as_str() { "at_most" => GoalDirection::AtMost, _ => GoalDirection::AtLeast },
                amount: row.get(4)?,
                unit: match unit.as_str() { "kcal" => GoalUnit::Kcal, "mg" => GoalUnit::Mg, "mcg" => GoalUnit::Mcg, _ => GoalUnit::G },
                period: match period.as_str() { "week" => GoalPeriod::Week, "month" => GoalPeriod::Month, _ => GoalPeriod::Day },
                created_at: row.get(7)?, updated_at: row.get(8)?,
            })
        }).map_err(|_| ApiError::InternalError)?
        .collect::<Result<Vec<_>, _>>().map_err(|_| ApiError::InternalError)?;

        let mut stmt = conn
            .prepare("SELECT key, value, updated_at FROM story")
            .map_err(|_| ApiError::InternalError)?;
        let story: Vec<StoryFlag> = stmt.query_map([], |row| {
            Ok(StoryFlag {
                key: row.get(0)?,
                value: row.get::<_, i32>(1)? != 0,
                updated_at: row.get(2)?,
            })
        }).map_err(|_| ApiError::InternalError)?
        .collect::<Result<Vec<_>, _>>().map_err(|_| ApiError::InternalError)?;

        Ok(SyncDumpResponse { foods, diary_entries, recipes, recipe_ingredients, goals, story })
    })
}

pub fn push(db: &Database, payload: SyncPushPayload) -> Result<SyncPushResponse, ApiError> {
    db.with_conn(|conn| {
        for food in &payload.foods {
            let nutrients_json = serde_json::to_string(&food.nutrients).unwrap_or_default();
            conn.execute(
                "INSERT INTO food (id, name, kcal, protein, fat, carbs, nutrients_json, package_weight, is_recipe, recipe_id, archived, is_restaurant, created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)
                 ON CONFLICT(id) DO UPDATE SET
                   name=excluded.name, kcal=excluded.kcal, protein=excluded.protein,
                   fat=excluded.fat, carbs=excluded.carbs, nutrients_json=excluded.nutrients_json,
                   package_weight=excluded.package_weight, is_recipe=excluded.is_recipe,
                   recipe_id=excluded.recipe_id, archived=excluded.archived,
                   is_restaurant=excluded.is_restaurant, updated_at=excluded.updated_at
                 WHERE excluded.updated_at > food.updated_at",
                rusqlite::params![
                    food.id, food.name, food.kcal, food.protein, food.fat, food.carbs,
                    nutrients_json, food.package_weight,
                    food.is_recipe as i32, food.recipe_id, food.archived as i32,
                    food.is_restaurant as i32,
                    food.created_at, food.updated_at,
                ],
            ).map_err(|_| ApiError::InternalError)?;
        }

        for entry in &payload.diary_entries {
            if entry.deleted {
                conn.execute(
                    "UPDATE diary SET deleted=1, updated_at=?1 WHERE id=?2 AND updated_at < ?1",
                    rusqlite::params![entry.updated_at, entry.id],
                ).map_err(|_| ApiError::InternalError)?;
            } else {
                conn.execute(
                    "INSERT INTO diary (id, food_id, date, time, grams, meal_label, waste_grams, created_at, updated_at)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
                     ON CONFLICT(id) DO UPDATE SET
                       food_id=excluded.food_id, date=excluded.date, time=excluded.time,
                       grams=excluded.grams, meal_label=excluded.meal_label,
                       waste_grams=excluded.waste_grams, updated_at=excluded.updated_at
                     WHERE excluded.updated_at > diary.updated_at",
                    rusqlite::params![
                        entry.id, entry.food_id, entry.date, entry.time,
                        entry.grams, entry.meal_label, entry.waste_grams,
                        entry.created_at, entry.updated_at,
                    ],
                ).map_err(|_| ApiError::InternalError)?;
            }
        }

        for recipe in &payload.recipes {
            conn.execute(
                "INSERT INTO recipe (id, name, notes, total_grams, finalized, food_id, created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
                 ON CONFLICT(id) DO UPDATE SET
                   name=excluded.name, notes=excluded.notes, total_grams=excluded.total_grams,
                   finalized=excluded.finalized, food_id=excluded.food_id, updated_at=excluded.updated_at
                 WHERE excluded.updated_at > recipe.updated_at",
                rusqlite::params![
                    recipe.id, recipe.name, recipe.notes, recipe.total_grams,
                    recipe.finalized as i32, recipe.food_id, recipe.created_at, recipe.updated_at,
                ],
            ).map_err(|_| ApiError::InternalError)?;
        }

        for ing in &payload.recipe_ingredients {
            conn.execute(
                "INSERT INTO recipe_ingredient (id, recipe_id, food_id, grams, created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)
                 ON CONFLICT(id) DO UPDATE SET
                   recipe_id=excluded.recipe_id, food_id=excluded.food_id,
                   grams=excluded.grams, updated_at=excluded.updated_at
                 WHERE excluded.updated_at > recipe_ingredient.updated_at",
                rusqlite::params![
                    ing.id, ing.recipe_id, ing.food_id, ing.grams, ing.created_at, ing.updated_at,
                ],
            ).map_err(|_| ApiError::InternalError)?;
        }

        for goal in &payload.goals {
            let dir = match goal.direction { GoalDirection::AtLeast => "at_least", GoalDirection::AtMost => "at_most" };
            let unit = match goal.unit { GoalUnit::Kcal => "kcal", GoalUnit::G => "g", GoalUnit::Mg => "mg", GoalUnit::Mcg => "mcg" };
            let period = match goal.period { GoalPeriod::Day => "day", GoalPeriod::Week => "week", GoalPeriod::Month => "month" };
            conn.execute(
                "INSERT INTO goal (id, nutrient, key, direction, amount, unit, period, created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
                 ON CONFLICT(id) DO UPDATE SET
                   nutrient=excluded.nutrient, key=excluded.key, direction=excluded.direction,
                   amount=excluded.amount, unit=excluded.unit, period=excluded.period, updated_at=excluded.updated_at
                 WHERE excluded.updated_at > goal.updated_at",
                rusqlite::params![
                    goal.id, goal.nutrient, goal.key, dir, goal.amount, unit, period,
                    goal.created_at, goal.updated_at,
                ],
            ).map_err(|_| ApiError::InternalError)?;
        }

        for flag in &payload.story {
            conn.execute(
                "INSERT INTO story (key, value, updated_at)
                 VALUES (?1, ?2, ?3)
                 ON CONFLICT(key) DO UPDATE SET
                   value=excluded.value, updated_at=excluded.updated_at
                 WHERE excluded.updated_at > story.updated_at",
                rusqlite::params![flag.key, flag.value as i32, flag.updated_at],
            ).map_err(|_| ApiError::InternalError)?;
        }

        Ok(SyncPushResponse { conflicts: None })
    })
}

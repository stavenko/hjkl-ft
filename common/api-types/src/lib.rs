use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

pub mod nutrient_key;

pub const CONTENT_TYPE: &str = "application/x-postcard";

pub fn encode<T: Serialize>(value: &T) -> Vec<u8> {
    postcard::to_allocvec(value).expect("postcard encode failed")
}

pub fn decode<'a, T: Deserialize<'a>>(bytes: &'a [u8]) -> Result<T, postcard::Error> {
    postcard::from_bytes(bytes)
}

// --- Response envelope ---

#[derive(Debug, Serialize, Deserialize)]
pub enum ApiResponseEnvelope<T> {
    Ok(T),
    Err(ApiError),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ApiError {
    NotFound,
    BadRequest(String),
    InternalError,
}

// --- Domain models ---

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Food {
    pub id: String,
    pub name: String,
    pub kcal: f64,
    pub protein: f64,
    pub fat: f64,
    pub carbs: f64,
    /// Custom nutrients: name -> value (in the unit defined by the goal)
    pub nutrients: BTreeMap<String, f64>,
    /// Net weight of product in package (grams), if known
    pub package_weight: Option<f64>,
    pub is_recipe: bool,
    pub recipe_id: Option<String>,
    pub archived: bool,
    /// Restaurant / eaten-out food: excluded from recipe ingredients, carries a
    /// +20% calorie surcharge, and is shown with a dashed marker in lists.
    #[serde(default)]
    pub is_restaurant: bool,
    pub created_at: String,
    pub updated_at: String,
}

impl Food {
    /// Calories accounted for this food. Restaurant meals carry a +20% surcharge
    /// (kitchens add hidden fats/oils to almost any dish).
    pub fn effective_kcal(&self) -> f64 {
        if self.is_restaurant { self.kcal * 1.2 } else { self.kcal }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Recipe {
    pub id: String,
    pub name: String,
    pub notes: Option<String>,
    pub total_grams: Option<f64>,
    pub finalized: bool,
    pub food_id: Option<String>,
    pub ingredients: Vec<RecipeIngredient>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecipeIngredient {
    pub id: String,
    pub recipe_id: String,
    pub food_id: String,
    pub grams: f64,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiaryEntry {
    pub id: String,
    pub food_id: String,
    pub date: String,
    pub time: Option<String>,
    pub grams: f64,
    /// Inedible waste (bones, pits, …) in grams. Calories count `grams - waste_grams`.
    #[serde(default)]
    pub waste_grams: f64,
    pub meal_label: Option<String>,
    #[serde(default)]
    pub deleted: bool,
    pub created_at: String,
    pub updated_at: String,
}

/// Draft from AI lookup — not yet a confirmed Food.
/// When added to diary, a Food is created and food_id is set.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FoodDraft {
    pub id: String,
    pub name: String,
    pub kcal: f64,
    pub protein: f64,
    pub fat: f64,
    pub carbs: f64,
    pub nutrients: BTreeMap<String, f64>,
    pub package_weight: Option<f64>,
    /// Once added to diary, points to the created Food
    pub food_id: Option<String>,
    pub created_at: String,
}

impl FoodDraft {
    pub fn to_food(&self) -> Food {
        Food {
            id: self.id.clone(),
            name: self.name.clone(),
            kcal: self.kcal,
            protein: self.protein,
            fat: self.fat,
            carbs: self.carbs,
            nutrients: self.nutrients.clone(),
            package_weight: self.package_weight,
            is_recipe: false,
            recipe_id: None,
            archived: false,
            is_restaurant: false,
            created_at: self.created_at.clone(),
            updated_at: String::new(),
        }
    }
}

// --- Request/Response DTOs ---

// Food
#[derive(Debug, Serialize, Deserialize)]
pub struct CreateFoodInput {
    pub name: String,
    pub kcal: f64,
    pub protein: f64,
    pub fat: f64,
    pub carbs: f64,
    pub nutrients: BTreeMap<String, f64>,
    pub package_weight: Option<f64>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct UpdateFoodInput {
    pub id: String,
    pub name: String,
    pub kcal: f64,
    pub protein: f64,
    pub fat: f64,
    pub carbs: f64,
    pub nutrients: BTreeMap<String, f64>,
    pub package_weight: Option<f64>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct DeleteInput {
    pub id: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ArchiveFoodInput {
    pub id: String,
    pub archived: bool,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ListFoodOutput {
    pub foods: Vec<Food>,
}

// AI Lookup
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NutrientSpec {
    pub key: String,
    pub name: String,
    pub unit_label: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct AiLookupInput {
    pub name: String,
    pub custom_nutrients: Vec<NutrientSpec>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AiValueWithUnit {
    pub value: f64,
    pub unit: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AiNutrientDetail {
    pub min_value: AiValueWithUnit,
    pub max_value: AiValueWithUnit,
    pub recommended: AiValueWithUnit,
    pub comment: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AiLookupOutput {
    pub name: Option<String>,
    pub kcal: AiNutrientDetail,
    pub protein: AiNutrientDetail,
    pub fat: AiNutrientDetail,
    pub carbs: AiNutrientDetail,
    pub nutrients: BTreeMap<String, AiNutrientDetail>,
    pub package_weight: Option<f64>,
}

// AI Vision
#[derive(Debug, Serialize, Deserialize)]
pub struct AiVisionInput {
    pub images: Vec<String>,
    pub custom_nutrients: Vec<NutrientSpec>,
}

// Recipe
#[derive(Debug, Serialize, Deserialize)]
pub struct CreateRecipeInput {
    pub name: String,
    pub notes: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct AddIngredientInput {
    pub recipe_id: String,
    pub food_id: String,
    pub grams: f64,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct UpdateIngredientInput {
    pub id: String,
    pub grams: f64,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct UpdateRecipeInput {
    pub id: String,
    pub name: String,
    pub notes: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct FinalizeRecipeInput {
    pub id: String,
    pub total_grams: f64,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct GetRecipeInput {
    pub id: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct CloneRecipeInput {
    pub id: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ListRecipeOutput {
    pub recipes: Vec<Recipe>,
}

// Diary
#[derive(Debug, Serialize, Deserialize)]
pub struct CreateDiaryEntryInput {
    pub food_id: String,
    pub date: String,
    pub time: Option<String>,
    pub grams: f64,
    pub meal_label: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct UpdateDiaryEntryInput {
    pub id: String,
    pub food_id: String,
    pub date: String,
    pub time: Option<String>,
    pub grams: f64,
    pub meal_label: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ListDiaryInput {
    pub date: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ListDiaryOutput {
    pub entries: Vec<DiaryEntry>,
}

// --- Goals ---

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub enum GoalDirection {
    AtLeast,
    AtMost,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub enum GoalPeriod {
    Day,
    Week,
    Month,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub enum GoalUnit {
    Kcal,
    G,
    Mg,
    Mcg,
}

impl GoalUnit {
    pub fn label(self) -> &'static str {
        match self {
            GoalUnit::Kcal => "kcal",
            GoalUnit::G => "g",
            GoalUnit::Mg => "mg",
            GoalUnit::Mcg => "µg",
        }
    }
}

/// Well-known nutrients with fixed units.
pub enum KnownNutrient {
    Calories,
    Protein,
    Fat,
    Carbs,
}

impl KnownNutrient {
    pub fn label(&self) -> &'static str {
        match self {
            KnownNutrient::Calories => "Calories",
            KnownNutrient::Protein => "Protein",
            KnownNutrient::Fat => "Fat",
            KnownNutrient::Carbs => "Carbs",
        }
    }

    pub fn unit(&self) -> GoalUnit {
        match self {
            KnownNutrient::Calories => GoalUnit::Kcal,
            _ => GoalUnit::G,
        }
    }

    pub const ALL: &'static [KnownNutrient] = &[
        KnownNutrient::Calories,
        KnownNutrient::Protein,
        KnownNutrient::Fat,
        KnownNutrient::Carbs,
    ];
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Goal {
    pub id: String,
    pub nutrient: String,
    pub key: String,
    pub direction: GoalDirection,
    pub amount: f64,
    pub unit: GoalUnit,
    pub period: GoalPeriod,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct CreateGoalInput {
    pub nutrient: String,
    pub direction: GoalDirection,
    pub amount: f64,
    pub unit: GoalUnit,
    pub period: GoalPeriod,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct UpdateGoalInput {
    pub id: String,
    pub nutrient: String,
    pub direction: GoalDirection,
    pub amount: f64,
    pub unit: GoalUnit,
    pub period: GoalPeriod,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ListGoalOutput {
    pub goals: Vec<Goal>,
}

// --- Weight tracking ---

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WeightEntry {
    pub id: String,
    pub date: String,
    pub weight_kg: f64,
    pub no_water: bool,
    pub no_food: bool,
    pub no_wash: bool,
    pub used_toilet: bool,
    pub morning: bool,
    pub created_at: String,
    pub updated_at: String,
}

// --- Step tracking ---

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StepEntry {
    pub id: String,
    pub date: String,
    pub steps: u32,
    pub created_at: String,
    pub updated_at: String,
}

// --- Sync types ---

/// A story-progress flag, synced last-writer-wins by `updated_at` (like foods/goals).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoryFlag {
    pub key: String,
    pub value: bool,
    #[serde(default)]
    pub updated_at: String,
}

/// An explicit deletion record (tombstone). Deleting an entity on the client
/// produces one of these; it is synced like any other row and APPLIED on every
/// device (remove the target locally). The backend never hard-deletes entities —
/// it only accumulates these records — so a deletion always propagates and can't
/// be undone by a stale copy.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeletionRecord {
    /// Unique id of this deletion record.
    pub id: String,
    /// Store/entity kind of the deleted target (e.g. "diary", "goals").
    pub kind: String,
    /// Id of the deleted entity.
    pub target_id: String,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncDumpResponse {
    pub foods: Vec<Food>,
    pub diary_entries: Vec<DiaryEntry>,
    pub recipes: Vec<Recipe>,
    pub recipe_ingredients: Vec<RecipeIngredient>,
    pub goals: Vec<Goal>,
    #[serde(default)]
    pub story: Vec<StoryFlag>,
    #[serde(default)]
    pub weight_entries: Vec<WeightEntry>,
    #[serde(default)]
    pub step_entries: Vec<StepEntry>,
    #[serde(default)]
    pub deletions: Vec<DeletionRecord>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncPushPayload {
    pub foods: Vec<Food>,
    pub diary_entries: Vec<DiaryEntry>,
    pub recipes: Vec<Recipe>,
    pub recipe_ingredients: Vec<RecipeIngredient>,
    pub goals: Vec<Goal>,
    #[serde(default)]
    pub story: Vec<StoryFlag>,
    #[serde(default)]
    pub weight_entries: Vec<WeightEntry>,
    #[serde(default)]
    pub step_entries: Vec<StepEntry>,
    #[serde(default)]
    pub deletions: Vec<DeletionRecord>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncPushResponse {
    pub conflicts: Option<SyncDumpResponse>,
}

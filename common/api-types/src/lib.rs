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
    /// AI-assigned tag: vegetable / fruit (fresh, cooked, or an obvious veg/fruit
    /// dish; not cereals, meat, fish, dairy, sweets or drinks). `None` = not yet
    /// classified. Classified in the background when the food is logged. Replaces
    /// the summary-time index judging as the stored source of truth.
    #[serde(default)]
    pub is_veg_fruit: Option<bool>,
    /// AI-assigned tag: rich source of HEME iron — печень и субпродукты, красное
    /// мясо, моллюски. `None` = not yet classified.
    ///
    /// Признак про ЦЕЛЬ («съешьте три порции»), в отличие от соседнего
    /// `is_red_meat`, который про ограничение. Продукт бывает и тем и другим сразу:
    /// говядина — и источник гема, и красное мясо, и это не противоречие, а два
    /// разных разговора о ней.
    #[serde(default)]
    pub is_heme: Option<bool>,
    /// AI-assigned tag: КРАСНОЕ МЯСО — МЫШЕЧНАЯ ткань млекопитающих (говядина,
    /// свинина, баранина, козлятина, дичь). `None` = ещё не спрашивали.
    ///
    /// Язык и сердце ВХОДЯТ, хотя это органы: по ткани они мышцы, и по составу — то же
    /// мясо с тем же гемовым железом и тем же жиром, поэтому недельные граммы они
    /// тратят. Печень, почки, лёгкие и требуха не входят: они не мышцы. Птица не
    /// входит никакой частью, включая куриные сердечки.
    ///
    /// Страуса в категориях больше нет: строка про него была экзотикой на один продукт
    /// из тысячи, а работала карманом для всего незнакомого — «Голец» модель объявила
    /// мясом страуса, и съеденная рыба дала пик в недельной планке.
    ///
    /// Признак про ограничение: недельная планка в 700 г сырого (`services::red_meat`).
    /// Раньше такой флаг уже был и был снят вместе со своим классификатором —
    /// вернулся, когда у него появилась своя неделя и своя шкала.
    #[serde(default)]
    pub is_red_meat: Option<bool>,
    /// AI-assigned tag: мясо ГЛУБОКОЙ ПЕРЕРАБОТКИ — колбасы, сосиски, ветчина,
    /// бекон, ливерные колбасы и прочее консервированное нитритами. `None` = ещё
    /// не спрашивали.
    ///
    /// Отдельно от `is_red_meat`, потому что связь с онкологией у переработанного
    /// мяса своя и меряется иначе: не граммами в неделю, а тем, как часто человек
    /// его ест. При этом оно ОСТАЁТСЯ красным мясом и тратит те же недельные
    /// граммы — один продукт отвечает по обоим счетам.
    #[serde(default)]
    pub is_processed_meat: Option<bool>,
    /// AI-assigned tag: жир продукта заключён в ЦЕЛУЮ молочно-жировую глобулу.
    ///
    /// Мембрана глобулы рвётся только сбиванием: в масле, топлёном масле и
    /// безводном молочном жире её нет, а в молоке, сливках, сыре, твороге,
    /// йогурте и кефире она цела. Разница не косметическая — при одинаковых
    /// жирных кислотах жир в целой глобуле ведёт себя иначе (сыр против масла:
    /// −6.5 % ЛПНП), поэтому такой жир не участвует в индикаторе баланса.
    ///
    /// `None` — ещё не классифицировано.
    #[serde(default)]
    pub is_milk_globule: Option<bool>,
    /// AI-assigned tag: продукт — ЯЙЦО ПТИЦЫ. `None` = ещё не спрашивали.
    ///
    /// Признак снимался вместе с удалённой «Оценкой» и стирался из базы миграцией
    /// 10 — вернулся отдельным узлом конвейера. Считается только само яйцо, чьё бы
    /// оно ни было и как бы ни приготовлено, а также желток и белок порознь. Блюдо,
    /// в котором яйцо лишь одно из многого, сюда не идёт, и икра тоже: она рыбья.
    #[serde(default)]
    pub is_egg: Option<bool>,
    /// Iron in mg per 100 g. Filled by the DEDICATED iron pass, not by the general
    /// nutrient enricher — iron is judged together with how well it is absorbed, so
    /// it deliberately does NOT live in `nutrients` (which is a plain amount map
    /// rendered in forms and badges). `None` = not looked up yet.
    #[serde(default)]
    pub iron_mg: Option<f64>,
    /// The FRACTION of this food's iron the body actually takes up (0…1). Heme iron
    /// (red meat, liver, shellfish) absorbs several times better than non-heme iron
    /// (legumes, seeds, greens), so the same milligrams are worth very different
    /// amounts depending on the source. Filled together with `iron_mg` by the same
    /// pass. `None` = not looked up yet.
    #[serde(default)]
    pub iron_absorption: Option<f64>,
    /// Профиль жира этого продукта. `None` — ещё не спрашивали.
    #[serde(default)]
    pub fat_profile: Option<FatProfile>,
    /// ТОЛЬКО У БЛЮД: профиль той части жира, что участвует в индикаторе баланса —
    /// без ингредиентов с целой молочно-жировой глобулой, в долях от общего жира
    /// блюда. У обычного продукта `None`: там тот же вопрос решает флаг
    /// `is_milk_globule`, целиком отбрасывающий его жир.
    #[serde(default)]
    pub balance_fat_profile: Option<FatProfile>,
    pub created_at: String,
    pub updated_at: String,
}

/// Состав жира продукта — ДОЛИ от его собственного жира, в процентах.
///
/// Доли, а не граммы на 100 г, намеренно. Во-первых, `Food::fat` у нас уже есть, и
/// он получен не из этого запроса — граммы кислот считаются умножением, а не берутся
/// у модели второй раз. Во-вторых, доли не зависят от разбавления, уварки и способа
/// готовки: профиль — свойство ТИПА жира, а не блюда, поэтому подсолнечное масло и
/// соус на нём делят один профиль. У железа отдельных строк для сырого и готового
/// продукта не хватило, и варёная чечевица систематически завышалась.
///
/// Три доли не обязаны давать 100: остаток веса жира — глицерин. Требование «ровно
/// сто» на замере превращало младшую фракцию в мусорную корзину — сливочное масло
/// получало вдвое завышенную ПНЖК.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct FatProfile {
    /// Насыщенные, % от общего жира.
    pub sfa_pct: f64,
    /// Мононенасыщенные, % от общего жира.
    pub mufa_pct: f64,
    /// Полиненасыщенные, % от общего жира.
    pub pufa_pct: f64,
    /// EPA + DHA вместе, % от общего жира. Часть `pufa_pct`.
    ///
    /// Альфа-линоленовой (АЛК) здесь НЕТ намеренно: её считал единственный
    /// индикатор, который решено не выпускать, а спрашивать у модели то, чего никто
    /// не читает, незачем.
    pub epa_dha_pct: f64,
}

/// Жирные кислоты в ГРАММАХ. Складываются: за день, за неделю, по составу блюда.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct FattyAcids {
    pub sfa_g: f64,
    pub mufa_g: f64,
    pub pufa_g: f64,
    pub epa_dha_g: f64,
}

impl std::ops::Add for FattyAcids {
    type Output = Self;
    fn add(self, o: Self) -> Self {
        Self {
            sfa_g: self.sfa_g + o.sfa_g,
            mufa_g: self.mufa_g + o.mufa_g,
            pufa_g: self.pufa_g + o.pufa_g,
            epa_dha_g: self.epa_dha_g + o.epa_dha_g,
        }
    }
}

impl std::ops::AddAssign for FattyAcids {
    fn add_assign(&mut self, o: Self) {
        *self = *self + o;
    }
}

impl FattyAcids {
    /// Умножить на вес: доли считаются на 100 г, поэтому `grams / 100.0`.
    pub fn scaled(self, factor: f64) -> Self {
        Self {
            sfa_g: self.sfa_g * factor,
            mufa_g: self.mufa_g * factor,
            pufa_g: self.pufa_g * factor,
            epa_dha_g: self.epa_dha_g * factor,
        }
    }

    /// Есть ли вообще данные о жире. Ноль по всем трём фракциям означает НЕ «жир
    /// плохой», а «не знаем»: у продуктов ещё нет профиля, либо в этот день не ели
    /// ничего жирного. Судить такую неделю нельзя — это тот же случай, что и неделя
    /// без дневника.
    pub fn has_data(&self) -> bool {
        self.sfa_g + self.mufa_g + self.pufa_g > 0.0
    }

    /// Отношение (МНЖК+ПНЖК)/НЖК. `None`, когда насыщенных нет: делить не на что, и
    /// «бесконечно хорошо» — не значение, которое можно показать шкалой.
    pub fn unsat_to_sat(&self) -> Option<f64> {
        (self.sfa_g > 0.0).then(|| (self.mufa_g + self.pufa_g) / self.sfa_g)
    }
}

impl FatProfile {
    /// Граммы кислот на 100 г продукта: доля × собственный жир.
    pub fn per_100g(&self, fat_per_100g: f64) -> FattyAcids {
        let g = |pct: f64| fat_per_100g * pct / 100.0;
        FattyAcids {
            sfa_g: g(self.sfa_pct),
            mufa_g: g(self.mufa_pct),
            pufa_g: g(self.pufa_pct),
            epa_dha_g: g(self.epa_dha_pct),
        }
    }
}

impl Food {
    /// Жирные кислоты этого продукта в граммах на 100 г. `None`, пока профиль не
    /// спрошен. Ноль жира — полноценный ответ, а не пробел: у сахара кислот нет.
    pub fn fatty_acids_per_100g(&self) -> Option<FattyAcids> {
        Some(self.fat_profile?.per_100g(self.fat))
    }

    /// Кислоты, участвующие в ИНДИКАТОРЕ БАЛАНСА, в граммах на 100 г.
    ///
    /// У блюда берётся `balance_fat_profile` — состав уже разобран по ингредиентам.
    /// У обычного продукта вопрос решается флагом: глобульный не даёт ничего,
    /// остальные — весь свой жир. `None` значит «не знаем», а не «ноль».
    pub fn balance_acids_per_100g(&self) -> Option<FattyAcids> {
        if self.is_recipe {
            return Some(self.balance_fat_profile?.per_100g(self.fat));
        }
        match self.is_milk_globule {
            Some(true) => Some(FattyAcids::default()),
            Some(false) => self.fatty_acids_per_100g(),
            None => None,
        }
    }

    /// Iron this food actually delivers, in mg per 100 g: amount × absorption.
    /// `None` until the dedicated iron pass has filled BOTH fields.
    pub fn absorbed_iron_mg_per_100g(&self) -> Option<f64> {
        match (self.iron_mg, self.iron_absorption) {
            (Some(mg), Some(a)) if mg >= 0.0 && (0.0..=1.0).contains(&a) => Some(mg * a),
            _ => None,
        }
    }
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
    /// Set when this dish is cooked AGAIN: the id of the newer recipe that
    /// inherited from (superseded) this one. A recipe with `superseded_by = Some(_)`
    /// has a continuation and is hidden from the recipes list AND the diary food
    /// search; recipes with `None` (no successor yet) are the ones shown. Explicit
    /// parentage — never inferred from the (mutable) name. `serde(default)` so old
    /// recipes load as `None` (no migration).
    #[serde(default)]
    pub superseded_by: Option<String>,
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
            is_veg_fruit: None,
            is_heme: None,
            is_milk_globule: None,
            is_red_meat: None,
            is_processed_meat: None,
            is_egg: None,
            fat_profile: None,
            balance_fat_profile: None,
            iron_mg: None,
            iron_absorption: None,
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
    /// True when the name came from a food PHOTO where the estimated grams are the
    /// COOKED / as-served portion on the plate. The nutrition lookup must then
    /// value the food in its cooked/ready-to-eat state (e.g. boiled pasta ~130
    /// kcal/100 g), NOT the raw/dry product (~350) — otherwise a cooked portion
    /// weight × raw density over-counts calories ~2–3×.
    #[serde(default)]
    pub as_served: bool,
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

// AI Food-items (dish photo → list of foods). Uses the same on-prem Qwen2.5-VL
// through the ocr-queue as label OCR, but with a "food_items" job kind that
// returns a LIST of detected foods instead of one per-100g label. See
// docs/food-photo-recognition.md.
#[derive(Debug, Serialize, Deserialize)]
pub struct AiFoodItemsInput {
    pub images: Vec<String>,
}

/// One food the vision model detected in a dish photo. `grams` is an estimate
/// (always editable downstream). `inferred = true` marks an auto-added hidden-fat
/// row (cooking oil / mayonnaise) the model added on top of the visible food — a
/// hypothesis, shown explicitly and removable, never baked silently into a total.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DetectedFood {
    pub name: String,
    pub grams: f64,
    pub confidence: f64,
    #[serde(default)]
    pub inferred: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AiFoodItemsOutput {
    pub items: Vec<DetectedFood>,
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
    Steps,
}

impl GoalUnit {
    pub fn label(self) -> &'static str {
        match self {
            GoalUnit::Kcal => "kcal",
            GoalUnit::G => "g",
            GoalUnit::Mg => "mg",
            GoalUnit::Mcg => "µg",
            GoalUnit::Steps => "steps",
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

impl Goal {
    /// A CUSTOM food nutrient shown as an extra field on the food editor and as a
    /// food badge: any nutrient goal that isn't one of the built-in KBJU fields
    /// (Calories/Protein/Fat/Carbs). The `goals` store holds ONLY food nutrients —
    /// the steps planka lives on the profile, not here — so no activity/store-type
    /// special-casing is needed.
    pub fn is_custom_nutrient(&self) -> bool {
        !matches!(self.nutrient.as_str(), "Calories" | "Protein" | "Fat" | "Carbs")
    }
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

// --- Planka history ---

/// Одна установка планки: С КАКОГО ДНЯ она действует и какая.
///
/// Планка — движущаяся величина: калорийную пересчитывает недельный цикл по тренду
/// веса, шаговую поднимает цвет индикатора. Без истории «какая планка была во
/// вторник» ответить нечем, и день, засчитанный тогда, приходится либо замораживать,
/// либо пересуживать по сегодняшней — а это переписывает прошлое.
///
/// Хранятся только планки, НЕ зависящие от личности: калории и шаги. Нормы белка,
/// овощей и кальция выводятся из профиля и в истории не нуждаются.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlankaEntry {
    /// `<kind>:<date>` — одна запись на вид и день установки.
    pub id: String,
    /// `calories` | `steps`.
    pub kind: String,
    /// Дата (YYYY-MM-DD), С КОТОРОЙ действует это значение.
    pub date: String,
    /// Значение: ккал/день для `calories`, шагов/день для `steps`.
    pub amount: f64,
    pub created_at: String,
    pub updated_at: String,
}

/// Планка, которую задал КУРАТОР, — и его же запрет автопересчёта.
///
/// Отдельная сущность, а не поле рядом с нашими значениями, ровно по одной
/// причине: действующая планка выбирается приоритетом. Есть запись куратора —
/// берётся она, нет — работает прежнее правило приложения (недельный пересчёт,
/// вывод из калорийной планки, константа). Смешай мы одно с другим, «вернуть как
/// было» после отвязки стало бы невозможно: неоткуда узнать, чьё это число.
///
/// Поэтому здесь НЕТ поля «кто поставил» — оно всегда куратор, иначе записи бы
/// не было.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CuratorPlankaRow {
    /// Ключ индикатора: `calories`, `steps`, `protein`, `veg_fruit`, `calcium`,
    /// `fiber`, `iron`, `heme`, `epa_dha`, `fat_ratio`, `red_meat`, `egg`.
    pub key: String,
    /// Значение в единицах этого индикатора. `None` — куратор тронул только
    /// замок, а число оставил наше.
    #[serde(default)]
    pub amount: Option<f64>,
    /// Запрет автопересчёта. Осмыслен лишь у выводимых величин — калории и шаги
    /// (недельный цикл), белок и клетчатка (от калорийной планки), овощи-фрукты
    /// (от пола), железо (от пола и возраста). У констант пересчитывать нечего.
    #[serde(default)]
    pub locked: bool,
    pub updated_at: String,
}

// --- Sync types ---

/// The synced user profile, modelled as a keyed singleton (one row, key
/// "profile") so it rides the same array-of-keyed-rows, LWW-by-`updated_at`
/// machinery as the other synced stores. All fields `#[serde(default)]` so old clients and
/// servers stay compatible.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProfileRow {
    pub key: String, // always "profile"
    #[serde(default)]
    pub sex: Option<String>, // "male" | "female" | None
    #[serde(default)]
    pub height_cm: Option<f64>,
    #[serde(default)]
    pub birth_year: Option<i32>,
    /// The course goal: "lose" | "maintain". None ⇒ default "lose".
    #[serde(default)]
    pub goal: Option<String>,
    /// First day of the current menstrual cycle (YYYY-MM-DD), if the user set it.
    #[serde(default)]
    pub cycle_start: Option<String>,
    /// The daily steps planka (activity target), set by the system on the activity
    /// week — NOT a food nutrient, so it lives here rather than in the `goals` store
    /// where every nutrient-iterating UI would pick it up. Not manually editable.
    #[serde(default)]
    pub steps_planka: Option<f64>,
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

/// One `app_flags` row: a per-user key/value flag. Cross-device flags (story
/// progress, indicator unlocks, letters, planka anchors) ride sync keyed by
/// `key`, LWW by `updated_at`; device-local keys (push subscription, caches,
/// migration markers) are filtered out client-side and never reach the server.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppFlagRow {
    pub key: String,
    pub value: String,
    /// RFC3339 write stamp; empty on legacy rows written before flags synced.
    #[serde(default)]
    pub updated_at: String,
}

/// One frozen indicator-day value. Computed ONCE — by the first device that had
/// the planka + diary data for that day — then synced like any other data, so
/// every other device APPLIES the ready value instead of recomputing its own.
/// Conflict (two devices computed independently while offline): the EARLIER
/// `computed_at` wins (first-writer-wins on the server).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IndDayRow {
    /// `"<indicator>:<date>"` — the sync key.
    pub id: String,
    /// protein | veg_fruit | steps | calories.
    pub indicator: String,
    pub date: String,
    pub value: f64,
    #[serde(default)]
    pub ratio: Option<f64>,
    /// RFC3339 freeze moment; empty on rows frozen before this field existed
    /// (sorts earliest, so pre-existing values keep priority).
    #[serde(default)]
    pub computed_at: String,
}

// ── Sync v2: incremental, journaled batches ─────────────────────────────────
// The CLIENT forms the change list (outbox of local mutations). A sync pushes
// them as ONE batch carrying the client's base version; the server appends the
// batch to a journal under `version = last + 1` — dumb storage, nothing else.
// A pull fetches only the journal tail newer than the client's version,
// applied strictly in order. A store starts uninitialized (pull = 409) until
// the first device migrates its local data in and confirms the batch count
// (/sync/v2/init); a later device without a version adopts the journal.

/// One journaled mutation. `store` is the client-side store name ("diary",
/// "foods", "goals", "profile", "weight_entries", "step_entries", "recipes",
/// "recipe_ingredients", "app_flags", "ind_days").
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncChange {
    pub store: String,
    /// "upsert" | "delete".
    pub op: String,
    /// Full row for an upsert.
    #[serde(default)]
    pub row: Option<serde_json::Value>,
    /// Row key for a delete (the store's key: id / date / key).
    #[serde(default)]
    pub id: Option<String>,
    /// EVENT time of the mutation (ms epoch, stamped by the device that made
    /// it). Used ONLY for automatic conflict resolution during the client-side
    /// merge (same-row edit/edit or edit/delete): the later event wins (ind_days
    /// keeps first-computation-wins). `updated_at` on rows is informational.
    #[serde(default)]
    pub ts: u64,
}

/// A journaled batch: the version it produced and the version the pushing
/// client had before it.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncBatch {
    pub version: u64,
    #[serde(default)]
    pub base_version: u64,
    pub changes: Vec<SyncChange>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncPushV2Request {
    pub base_version: u64,
    pub changes: Vec<SyncChange>,
}

fn default_accepted() -> bool {
    true
}

/// Push result. `accepted == false` ⇒ the batch was REJECTED because the client
/// pushed from a stale base (`base_version != server last`): pull the newer
/// batches, merge, and push again. Nothing is journaled on a rejection.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncPushV2Response {
    pub version: u64,
    #[serde(default = "default_accepted")]
    pub accepted: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncPullV2Request {
    #[serde(default)]
    pub since_version: u64,
}

/// Pull result: the ordered journal tail newer than `since_version`. The
/// journal is the ONLY data source — complete from the account's migration,
/// so a zero client (since 0) simply replays it from the beginning.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncPullV2Response {
    pub version: u64,
    #[serde(default)]
    pub batches: Vec<SyncBatch>,
}

//! Daily / weekly AI summaries. Computed via ai-worker (qwen3) and cached in
//! IndexedDB (`summaries` store). Daily summaries are available for any past day;
//! the week report is computed once the week has ended (the following Monday).

use chrono::{Datelike, Duration, NaiveDate};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::ai::AiPhase;
use super::{ai, db, i18n, local};

#[derive(Clone, Serialize, Deserialize)]
pub struct Summary {
    pub id: String,
    pub date: String,
    pub text: String,
    pub created_at: String,
}

/// One bullet of the daily summary. `url`, if present, is a source link that
/// must open in the system browser (e.g. the steps study).
#[derive(Clone, Serialize, Deserialize)]
pub struct SummaryItem {
    /// Curated text (from i18n), chosen by the item key the model selected.
    pub text: String,
    /// Source link (e.g. the steps study), or null if none.
    #[serde(default)]
    pub url: Option<String>,
}

/// Structured daily summary the model returns as JSON: what went well and what
/// to improve. Future story chapters will append conditional items (vegetables/
/// fruits from the fibre chapter, purines from protein, cholesterol from fat).
/// Deterministic per-day "Facts" block shown above the good/improve sections:
/// total КБЖУ (computed in Rust from the diary) plus the vegetable/fruit count
/// (the only AI-judged number, since foods carry no category). `None` when no
/// food was logged that day.
#[derive(Clone, Serialize, Deserialize)]
pub struct DayFacts {
    pub kcal: f64,
    pub protein: f64,
    pub fat: f64,
    pub carbs: f64,
    /// Total EATEN grams of vegetables / fruits today. Which logged foods count
    /// is judged by the model; the grams are summed deterministically from the
    /// diary.
    #[serde(default)]
    pub veg_fruit_grams: f64,
    /// A low-calorie snack food was logged today (name-matched in Rust).
    #[serde(default)]
    pub snack_logged: bool,
    /// A high-calorie drink was logged today (name-matched, >30 kcal/100g).
    #[serde(default)]
    pub high_cal_drink: bool,
    /// Protein (g) eaten in the evening (Dinner + NightSnack meal buckets).
    #[serde(default)]
    pub evening_protein_g: f64,
    /// Per-meal entry-count distribution, one entry per non-empty derived meal:
    /// (meal i18n key, count), in meal sort order.
    #[serde(default)]
    pub meal_distribution: Vec<(String, u32)>,
    /// The hidden daily calorie planka in effect when this day was assessed
    /// (chapter 3 / s1). 0.0 when no planka goal exists, in which case no
    /// planka facts/bands are emitted. > 0.0 means the day's kcal was compared
    /// against it.
    #[serde(default)]
    pub calorie_planka: f64,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct DaySummary {
    /// Deterministic totals (КБЖУ + veg/fruit count); None if no food logged.
    #[serde(default)]
    pub facts: Option<DayFacts>,
    /// What the user did well today (grounded in the facts only).
    #[serde(default)]
    pub good: Vec<SummaryItem>,
    /// Concrete, actionable suggestions for improvement (facts only).
    #[serde(default)]
    pub improve: Vec<SummaryItem>,
}

/// What the model returns: the KEYS of the catalog items that apply, plus the
/// vegetable/fruit count. The displayed item text is then taken verbatim from
/// i18n — the model judges WHICH points hold, but never writes the wording. The
/// schema stays trivial (string arrays + one integer, no maps / nullable /
/// $refs — which is what the strict model server accepts).
#[derive(Deserialize, JsonSchema)]
struct DayAssessmentAi {
    #[serde(default)]
    good: Vec<String>,
    #[serde(default)]
    improve: Vec<String>,
    /// Indices (into the prompt's numbered Foods list) of the logged foods that
    /// are vegetables or fruits. Empty if none.
    #[serde(default)]
    vegetable_fruit_indices: Vec<u32>,
}

/// Catalog of GOOD item keys → i18n text key. The order here is the display order.
const GOOD_ITEMS: &[(&str, &str)] = &[
    ("weight_steps", "summary.good_weight_steps"),
    ("diary", "summary.good_diary"),
    ("restaurant", "summary.good_restaurant"),
    ("snack", "summary.good_snack"),
    ("no_cal_drink", "summary.good_no_cal_drink"),
    ("evening_protein", "summary.good_evening_protein"),
    ("under_planka", "summary.good_under_planka"),
];

/// Catalog of IMPROVE item keys → i18n text key.
const IMPROVE_ITEMS: &[(&str, &str)] = &[
    ("weighing", "summary.improve_weighing"),
    ("steps", "summary.improve_steps"),
    ("drink", "summary.improve_drink"),
    ("over_planka", "summary.improve_over_planka"),
];

/// Parse a stored day summary. Tolerates ```json fences; returns None for legacy
/// free-text entries (the renderer then falls back to plain text).
pub fn parse_day(text: &str) -> Option<DaySummary> {
    let t = text.trim();
    let t = t.strip_prefix("```json").or_else(|| t.strip_prefix("```")).unwrap_or(t);
    let t = t.strip_suffix("```").unwrap_or(t).trim();
    serde_json::from_str(t).ok()
}

/// "Daily steps and health outcomes in adults: a systematic review and
/// dose-response meta-analysis", The Lancet Public Health (23 Jul 2025) — the
/// source for the ≥7000 steps/day recommendation. Opens in the system browser;
/// passed to the model so the steps "improve" item links to it verbatim.
const STEPS_STUDY_URL: &str =
    "https://www.thelancet.com/journals/lanpub/article/PIIS2468-2667(25)00164-1/fulltext";

fn now() -> String {
    chrono::Utc::now().to_rfc3339()
}

fn lang_name() -> &'static str {
    match i18n::get_lang() {
        i18n::Lang::Ru => "Russian",
        i18n::Lang::En => "English",
    }
}

// ---- Week date helpers (weeks are Mon–Sun) ----

pub fn week_start_of(date: &str) -> String {
    NaiveDate::parse_from_str(date, "%Y-%m-%d")
        .map(|d| (d - Duration::days(d.weekday().num_days_from_monday() as i64)).format("%Y-%m-%d").to_string())
        .unwrap_or_else(|_| date.to_string())
}

/// The Monday after `week_start` — when that week's report becomes available.
pub fn next_monday(week_start: &str) -> String {
    NaiveDate::parse_from_str(week_start, "%Y-%m-%d")
        .map(|d| (d + Duration::days(7)).format("%Y-%m-%d").to_string())
        .unwrap_or_else(|_| week_start.to_string())
}

/// True once the week containing `week_start` has fully ended.
pub fn week_ready(week_start: &str) -> bool {
    let today = chrono::Local::now().date_naive();
    NaiveDate::parse_from_str(&next_monday(week_start), "%Y-%m-%d")
        .map(|nm| today >= nm)
        .unwrap_or(false)
}

// ---- Storage ----

pub async fn get_day(date: &str) -> Option<Summary> {
    db::get("summaries", &format!("day:{date}")).await
}

pub async fn get_week(week_start: &str) -> Option<Summary> {
    db::get("summaries", &format!("week:{week_start}")).await
}

async fn store(id: String, date: String, text: String) -> Summary {
    let s = Summary { id, date, text, created_at: now() };
    db::put("summaries", &s).await;
    s
}

// ---- Context assembly ----

/// Last 15 weight + step entries, used by the weekly context.
async fn recent_body_block() -> String {
    let mut w = local::list_weight_entries().await;
    w.sort_by(|a, b| a.date.cmp(&b.date));
    let w: Vec<String> = w.iter().rev().take(15).rev()
        .map(|e| format!("{}: {:.1}kg", e.date, e.weight_kg)).collect();
    let mut s = local::list_step_entries().await;
    s.sort_by(|a, b| a.date.cmp(&b.date));
    let s: Vec<String> = s.iter().rev().take(15).rev()
        .map(|e| format!("{}: {}", e.date, e.steps)).collect();
    let mut out = String::new();
    if !w.is_empty() {
        out.push_str(&format!("\nRecent weight (last {}): {}", w.len(), w.join(", ")));
    }
    if !s.is_empty() {
        out.push_str(&format!("\nRecent steps (last {}): {}", s.len(), s.join(", ")));
    }
    out
}

async fn week_context(week_start: &str) -> Option<String> {
    let start = NaiveDate::parse_from_str(week_start, "%Y-%m-%d").ok()?;
    let mut any = false;
    let mut days = Vec::new();
    for i in 0..7 {
        let d = (start + Duration::days(i)).format("%Y-%m-%d").to_string();
        let diary = local::list_diary(&d).await;
        if diary.is_empty() {
            continue;
        }
        any = true;
        days.push(format!("{}: {} entries", d, diary.len()));
    }
    if !any {
        return None;
    }
    let mut ctx = format!("Week {} – {}\n", week_start, (start + Duration::days(6)).format("%Y-%m-%d"));
    ctx.push_str(&days.join("\n"));
    ctx.push_str(&recent_body_block().await);
    Some(ctx)
}

/// Grounded fact block for one day — the ONLY information the model may use to
/// build the assessment. Returns None if the day is completely empty (no diary,
/// no weight, no steps), in which case there's nothing to assess.
/// Grounded prompt text, the deterministic КБЖУ totals (None if no food was
/// logged; `(kcal, protein, fat, carbs)`), and the per-food EATEN grams in the
/// same order as the numbered "Foods" list in the prompt. The model returns the
/// indices it judges to be vegetables/fruits; we sum their grams here.
struct DayCtx {
    text: String,
    totals: Option<(f64, f64, f64, f64)>,
    food_grams: Vec<f64>,
    /// Deterministic chapter-2 facts, computed in Rust from the day's diary.
    snack_logged: bool,
    high_cal_drink: bool,
    evening_protein_g: f64,
    meal_distribution: Vec<(String, u32)>,
    /// Calorie planka in effect (0.0 = none); see DayFacts::calorie_planka.
    calorie_planka: f64,
}

async fn day_context(date: &str) -> Option<DayCtx> {
    let diary = local::list_diary(date).await;
    let weight = local::get_weight_for_date(date).await;
    let steps = local::get_steps_for_date(date).await;
    if diary.is_empty() && weight.is_none() && steps.is_none() {
        return None;
    }

    let mut ctx = format!("FACTS about the user's day ({date}). Use ONLY these.\n");
    ctx.push_str(&format!("- Diary entries logged: {}\n", diary.len()));

    let mut totals = None;
    let mut food_grams = Vec::new();
    let mut snack_logged = false;
    let mut high_cal_drink = false;
    let mut evening_protein_g = 0.0_f64;
    let mut meal_distribution: Vec<(String, u32)> = Vec::new();
    let mut calorie_planka = 0.0_f64;
    if diary.is_empty() {
        ctx.push_str("- Food logged: nothing was logged today.\n");
    } else {
        let fmap: std::collections::BTreeMap<String, api_types::Food> =
            local::list_foods().await.into_iter().map(|f| (f.id.clone(), f)).collect();
        let (mut kc, mut p, mut f, mut c) = (0.0, 0.0, 0.0, 0.0);
        let mut restaurant = 0usize;
        let mut lines = Vec::new();
        for e in &diary {
            if let Some(food) = fmap.get(&e.food_id) {
                let eaten = (e.grams - e.waste_grams).max(0.0);
                let factor = eaten / 100.0;
                let ek = food.effective_kcal() * factor;
                kc += ek;
                p += food.protein * factor;
                f += food.fat * factor;
                c += food.carbs * factor;
                if food.is_restaurant {
                    restaurant += 1;
                }
                if local::is_snack_food(food) {
                    snack_logged = true;
                }
                if local::is_high_cal_drink(food) {
                    high_cal_drink = true;
                }
                // Index in the prompt list == index in food_grams.
                lines.push(format!("  [{}] {} {:.0}g (~{:.0} kcal)", food_grams.len(), food.name, eaten, ek));
                food_grams.push(eaten);
            }
        }

        // Evening protein + per-meal distribution from the derived meal split.
        use crate::services::meal_split::{self, MealType};
        let groups = meal_split::group_by_meal(&diary);
        for grp in &groups {
            meal_distribution.push((grp.meal.i18n_key().to_string(), grp.entries.len() as u32));
            if grp.meal == MealType::Dinner || grp.meal == MealType::NightSnack {
                for e in &grp.entries {
                    if let Some(food) = fmap.get(&e.food_id) {
                        let eaten = (e.grams - e.waste_grams).max(0.0);
                        evening_protein_g += food.protein * eaten / 100.0;
                    }
                }
            }
        }

        ctx.push_str(&format!(
            "- Totals: {kc:.0} kcal, protein {p:.0}g, fat {f:.0}g, carbs {c:.0}g\n\
             - Restaurant-flagged items: {restaurant}\n\
             - Low-calorie snack logged: {snack}\n\
             - High-calorie drink logged: {drink}\n\
             - Evening protein (dinner + night): {ep:.0}g\n\
             - Meals (derived): {meals}\n\
             - Foods (index, name, eaten grams):\n{lines}\n",
            snack = if snack_logged { "yes" } else { "no" },
            drink = if high_cal_drink { "yes" } else { "no" },
            ep = evening_protein_g,
            meals = if meal_distribution.is_empty() {
                "none".to_string()
            } else {
                meal_distribution.iter()
                    .map(|(k, n)| format!("{k}={n}"))
                    .collect::<Vec<_>>().join(", ")
            },
            lines = lines.join("\n"),
        ));
        totals = Some((kc, p, f, c));

        // Calorie planka (chapter 3 / s1): a hidden AtMost "Calories" goal with
        // amount > 0. Compare the day's effective kcal to the planka P and emit a
        // grounded verdict so the model can select the right band. Never invent a
        // number — if there's no planka, add nothing.
        if let Some(p_goal) = local::list_goals().await.into_iter().find(|g| {
            g.nutrient == "Calories"
                && g.direction == api_types::GoalDirection::AtMost
                && g.amount > 0.0
        }) {
            calorie_planka = p_goal.amount;
            let p = calorie_planka;
            // > P: overate; < P-50: undereating too much; in [P-50, P]: good.
            let verdict = if kc > p {
                "OVER (ate above the planka — overate)"
            } else if kc < p - 50.0 {
                "UNDER (ate well below the planka — undereating too much)"
            } else {
                "GOOD (within the planka, did not exceed it)"
            };
            ctx.push_str(&format!(
                "- Calorie planka: {p:.0} kcal. Today's intake vs planka: {verdict}.\n"
            ));
        }
    }

    match &weight {
        Some(w) => {
            let quality = [w.no_water, w.no_food, w.no_wash, w.used_toilet, w.morning]
                .iter().filter(|&&b| b).count();
            ctx.push_str(&format!(
                "- Weight logged: yes, {:.1} kg. Weighing quality: {quality}/5 conditions met.\n",
                w.weight_kg,
            ));
        }
        None => ctx.push_str("- Weight logged: no.\n"),
    }
    match &steps {
        Some(s) => ctx.push_str(&format!("- Steps logged: yes, {} steps.\n", s.steps)),
        None => ctx.push_str("- Steps logged: no.\n"),
    }
    Some(DayCtx {
        text: ctx,
        totals,
        food_grams,
        snack_logged,
        high_cal_drink,
        evening_protein_g,
        meal_distribution,
        calorie_planka,
    })
}

// ---- Generation (AI, cached) ----

/// Drop the cached day assessment and generate a fresh one (the "Переделать
/// оценку" button). `on_token` drives the live thinking/answer UI.
pub async fn regenerate_day(
    date: &str,
    on_token: impl Fn(AiPhase) + Clone + 'static,
) -> Result<Option<Summary>, String> {
    db::delete("summaries", &format!("day:{date}")).await;
    ensure_day(date, on_token).await
}

/// Return the cached day assessment, or generate it with the model. The model
/// gets ONLY the grounded facts from `day_context` and is told not to invent
/// anything not stated — this is the guard against the earlier hallucination
/// ("you logged restaurant food" when none was). Returns Ok(None) when the day
/// is empty (nothing to assess).
/// Background tagging step: classify + cache snack tags for THIS day's foods that
/// aren't classified yet (a separate AI request), so `day_context`'s low-calorie
/// snack fact is language-independent. No-op when every food is already tagged.
/// FAIL LOUDLY: a classification error propagates and aborts report generation
/// (retried on the next activation) rather than silently mis-tagging.
async fn tag_day_snacks(date: &str) -> Result<(), String> {
    let diary = local::list_diary(date).await;
    if diary.is_empty() {
        return Ok(());
    }
    let fmap: std::collections::BTreeMap<String, api_types::Food> =
        local::list_foods().await.into_iter().map(|f| (f.id.clone(), f)).collect();

    // Distinct foods used today with no cached snack verdict yet.
    let mut seen = std::collections::HashSet::new();
    let mut pending: Vec<(String, String)> = Vec::new(); // (id, name)
    for e in &diary {
        if let Some(food) = fmap.get(&e.food_id) {
            if food.is_snack.is_none() && seen.insert(food.id.clone()) {
                pending.push((food.id.clone(), food.name.clone()));
            }
        }
    }
    if pending.is_empty() {
        return Ok(());
    }

    let names: Vec<String> = pending.iter().map(|(_, n)| n.clone()).collect();
    let verdicts = ai::classify_snacks(&names).await?; // guards count == names.len()
    let tags: Vec<(String, bool)> =
        pending.into_iter().map(|(id, _)| id).zip(verdicts).collect();
    local::cache_snack_tags(&tags).await;
    Ok(())
}

pub async fn ensure_day(
    date: &str,
    on_token: impl Fn(AiPhase) + Clone + 'static,
) -> Result<Option<Summary>, String> {
    if let Some(s) = get_day(date).await {
        return Ok(Some(s));
    }
    // Tag this day's foods (snack classification) in the background and cache the
    // verdicts BEFORE building the report, so the "low-calorie snack" fact is
    // language-independent rather than a Russian name match.
    tag_day_snacks(date).await?;
    let Some(dctx) = day_context(date).await else {
        return Ok(None);
    };

    // The model only SELECTS which catalog items apply (never writes wording) and
    // counts vegetables/fruits among the logged foods (the only judged number —
    // foods carry no category). The КБЖУ totals are computed in Rust, not here.
    let prompt = format!(
        "You are a nutrition and weight-loss coach evaluating a user's day. Decide which \
         assessment items apply, based STRICTLY on the FACTS below. Do NOT invent anything not \
         stated in the FACTS.\n\n\
         Return a JSON object with: two arrays of item KEYS (\"good\", \"improve\") and an array of \
         integers \"vegetable_fruit_indices\". Use ONLY the keys listed here, and include a key ONLY \
         when its condition holds:\n\n\
         GOOD items:\n\
         - \"weight_steps\": the user logged BOTH weight AND steps today.\n\
         - \"diary\": the user logged at least one food diary entry today.\n\
         - \"restaurant\": at least one logged food is flagged as restaurant food.\n\
         - \"snack\": the FACTS say a low-calorie snack was logged (yes).\n\
         - \"no_cal_drink\": food was logged AND the FACTS say NO high-calorie drink was logged (no).\n\
         - \"evening_protein\": the FACTS say evening protein is at least 30g.\n\
         - \"under_planka\": the FACTS contain a Calorie planka line AND the verdict is GOOD (within the planka).\n\n\
         IMPROVE items:\n\
         - \"weighing\": weight WAS logged AND weighing quality is below 5/5.\n\
         - \"steps\": steps were NOT logged, OR fewer than 7000 steps.\n\
         - \"drink\": the FACTS say a high-calorie drink was logged (yes).\n\
         - \"over_planka\": the FACTS contain a Calorie planka line AND the verdict is OVER or UNDER (not GOOD).\n\n\
         \"vegetable_fruit_indices\": from the numbered Foods list, the indices ([N]) of items that \
         are vegetables or fruits (fresh, cooked, or an obvious vegetable/fruit dish). Empty array \
         if none / no food. Do NOT include cereals, grains, meat, fish, dairy, sweets, or drinks.\n\n\
         Do NOT output any text, prose, or keys outside these lists.\n\n\
         {ctx}\n\n\
         Respond with ONLY a single minified JSON object, no markdown, exactly this shape:\n\
         {{\"good\":[\"weight_steps\"],\"improve\":[\"steps\"],\"vegetable_fruit_indices\":[0]}}",
        ctx = dctx.text,
    );

    let picked: DayAssessmentAi = ai::generate::<DayAssessmentAi>(prompt, on_token).await?;

    // Map selected keys → curated i18n text (verbatim), in catalog order. Unknown
    // keys are ignored; the steps item carries the study link.
    let to_items = |keys: &[String], catalog: &[(&str, &str)]| -> Vec<SummaryItem> {
        catalog
            .iter()
            .filter(|(k, _)| keys.iter().any(|p| p == k))
            .map(|(k, i18n_key)| SummaryItem {
                text: i18n::t(i18n_key).to_string(),
                url: (*k == "steps").then(|| STEPS_STUDY_URL.to_string()),
            })
            .collect()
    };
    // Facts: deterministic КБЖУ totals; veg/fruit GRAMS summed from the diary at
    // the indices the model flagged (ignoring any out-of-range index).
    let veg_fruit_grams: f64 = picked
        .vegetable_fruit_indices
        .iter()
        .filter_map(|&i| dctx.food_grams.get(i as usize))
        .sum();
    let facts = dctx.totals.map(|(kcal, protein, fat, carbs)| DayFacts {
        kcal,
        protein,
        fat,
        carbs,
        veg_fruit_grams,
        snack_logged: dctx.snack_logged,
        high_cal_drink: dctx.high_cal_drink,
        evening_protein_g: dctx.evening_protein_g,
        meal_distribution: dctx.meal_distribution.clone(),
        calorie_planka: dctx.calorie_planka,
    });
    let ds = DaySummary {
        facts,
        good: to_items(&picked.good, GOOD_ITEMS),
        improve: to_items(&picked.improve, IMPROVE_ITEMS),
    };

    let text = serde_json::to_string(&ds).map_err(|e| format!("serialize error: {e}"))?;
    Ok(Some(store(format!("day:{date}"), date.to_string(), text).await))
}

pub async fn ensure_week(week_start: &str) -> Option<Summary> {
    if let Some(s) = get_week(week_start).await {
        return Some(s);
    }
    if !week_ready(week_start) {
        return None;
    }
    let ctx = week_context(week_start).await?;
    let prompt = format!(
        "You are a supportive weight-loss coach. Write a SHORT weekly report (3–5 sentences) in \
         {lang}: overall trend, what went well, and one focus for next week. Do not invent numbers. \
         Plain text only.\n\n{ctx}",
        lang = lang_name(),
    );
    let text = ai::summarize(&prompt).await.ok()?;
    if text.is_empty() {
        return None;
    }
    Some(store(format!("week:{week_start}"), week_start.to_string(), text).await)
}

/// Ensure YESTERDAY's assessment exists. Called on every app ACTIVATION (launch +
/// foreground), so the report is prepared BEFORE the user opens the day rather
/// than generated on open. Best-effort, no UI; `ensure_day` no-ops when a report
/// already exists, so this only generates when there's none yet.
pub async fn ensure_yesterday() {
    let yesterday = (chrono::Local::now().date_naive() - Duration::days(1))
        .format("%Y-%m-%d")
        .to_string();
    let _ = ensure_day(&yesterday, |_| {}).await;
}

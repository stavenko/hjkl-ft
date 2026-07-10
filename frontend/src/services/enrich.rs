//! Background nutrient enrichment.
//!
//! The seven indicators need four extra nutrients on each food — calcium, iron,
//! omega-3, fiber — that the normal add-food flow doesn't collect. Rather than ask
//! for them at request time (which would overload the on-demand lookup), we fill
//! them in the BACKGROUND, one food at a time, reusing the EXISTING lookup format:
//! the model returns min / max / a recommended middle value + a comment per
//! nutrient (`ai::lookup` → `AiNutrientDetail`). We store the recommended value
//! (normalised to a canonical unit) into `Food.nutrients`, keyed by the display
//! name (same key the goals/indicators use).
//!
//! This runs inside the shared `classify` queue (see `classify::run_worker`) so
//! tags and nutrients for a food are filled by the same sequential background pass.

use std::collections::BTreeMap;

use api_types::Food;

use super::indicators::{N_CALCIUM, N_FIBER, N_IRON, N_OMEGA3};
use super::{ai, local};

#[derive(Clone, Copy)]
enum Unit {
    Mg,
    G,
}

impl Unit {
    fn label(self) -> &'static str {
        match self {
            Unit::Mg => "мг",
            Unit::G => "г",
        }
    }
}

/// The nutrients we enrich, with the canonical unit their stored value is in.
/// Keep these in sync with the indicator targets (`services::indicators`).
const TARGETS: &[(&str, Unit)] = &[
    (N_CALCIUM, Unit::Mg),
    (N_IRON, Unit::Mg),
    (N_OMEGA3, Unit::Mg),
    (N_FIBER, Unit::G),
];

/// True if `food` is missing any of the enriched nutrients yet.
pub fn needs_enrichment(food: &Food) -> bool {
    TARGETS.iter().any(|(name, _)| !food.nutrients.contains_key(*name))
}

/// The canonical unit label ("мг"/"г") for an enriched nutrient, or "" if the
/// nutrient isn't one we enrich (so the caller shows no unit).
pub fn nutrient_unit(name: &str) -> &'static str {
    TARGETS.iter().find(|(n, _)| *n == name).map(|(_, u)| u.label()).unwrap_or("")
}

/// The display names of the enriched nutrients (calcium/iron/omega-3/fiber).
pub fn nutrient_names() -> impl Iterator<Item = &'static str> {
    TARGETS.iter().map(|(n, _)| *n)
}

/// Fill the missing nutrients for `food` — ONE focused request per nutrient
/// (`ai::lookup_nutrient`), each asking for a single value in its canonical unit
/// with the food NAME as the only anchor. This deliberately does NOT reuse the
/// batched `ai::lookup`: asking for kcal + four nested nutrients at once makes qwen3
/// corrupt the JSON structure; one nutrient at a time keeps it focused.
///
/// Progress is cached as each nutrient arrives, so a later failure never loses the
/// values already fetched and they aren't re-requested. FAIL LOUDLY: the first
/// nutrient error is returned (the queue's `with_retries` retries the rest).
pub async fn enrich_food(food: &Food) -> Result<(), String> {
    for (name, unit) in TARGETS {
        if food.nutrients.contains_key(*name) {
            continue; // already enriched (e.g. by a previous partial pass)
        }
        let value = ai::lookup_nutrient(&food.name, name, unit.label()).await?;
        let mut one = BTreeMap::new();
        one.insert(name.to_string(), value);
        local::cache_food_nutrients(&food.id, one).await;
    }
    Ok(())
}

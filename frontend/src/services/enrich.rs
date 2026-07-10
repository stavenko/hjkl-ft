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

use api_types::{AiLookupInput, Food, NutrientSpec};

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

/// Look up the four nutrients for `food` (min/max/recommended + comment format) and
/// store the recommended values, normalised to the canonical unit. FAIL LOUDLY: a
/// lookup error is returned to the caller (logged, retried next sweep).
pub async fn enrich_food(food: &Food) -> Result<(), String> {
    let specs: Vec<NutrientSpec> = TARGETS
        .iter()
        .map(|(name, u)| NutrientSpec {
            key: api_types::nutrient_key::generate(name),
            name: name.to_string(),
            unit_label: u.label().to_string(),
        })
        .collect();
    let input = AiLookupInput { name: food.name.clone(), custom_nutrients: specs };
    let out = ai::lookup(&input, |_| {}).await?;

    let mut values = BTreeMap::new();
    for (name, canon) in TARGETS {
        // Store 0 when the model omits a nutrient, so the food counts as enriched
        // (present) and isn't re-queued forever.
        let v = out
            .nutrients
            .get(*name)
            .map(|d| normalize(d.recommended.value, &d.recommended.unit, *canon))
            .unwrap_or(0.0);
        values.insert(name.to_string(), v);
    }
    local::cache_food_nutrients(&food.id, values).await;
    Ok(())
}

/// Convert a value to the canonical unit. Weight units are understood in RU and EN;
/// an unrecognised unit is assumed to already be canonical (no scaling).
fn normalize(value: f64, unit: &str, canon: Unit) -> f64 {
    let u = unit.trim().to_lowercase();
    let mg = match u.as_str() {
        "kg" | "кг" => value * 1_000_000.0,
        "g" | "г" | "гр" | "gram" | "grams" | "грамм" => value * 1000.0,
        "mg" | "мг" => value,
        "mkg" | "mcg" | "µg" | "ug" | "мкг" => value * 0.001,
        _ => return value, // unknown unit → assume already canonical
    };
    match canon {
        Unit::Mg => mg,
        Unit::G => mg / 1000.0,
    }
}

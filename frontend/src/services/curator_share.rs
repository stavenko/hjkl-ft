//! Curator data-share: gather a requested dataset from the REAL local stores into
//! the typed data_share envelope (see the support-chat data-request protocol) and
//! serialize it to the JSON string carried in the message `payload`.
//!
//! NO sample data — every field is read from IndexedDB via the existing services.
//! Missing values are honest `null`s, never fabricated.

use serde_json::{json, Value};

use crate::services::weight_trend::{self, BalanceState, Direction, WeightTrend, DEFAULT_WINDOW_DAYS};
use crate::services::{i18n, local, profile};

/// The datasets a curator can request. Mirrors the protocol's `dataset` field.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Dataset {
    Body,
    Food,
    Weight,
    Steps,
    All,
}

impl Dataset {
    /// Parse the `dataset` value from a data_request envelope.
    pub fn from_str(s: &str) -> Option<Dataset> {
        Some(match s {
            "body" => Dataset::Body,
            "food" => Dataset::Food,
            "weight" => Dataset::Weight,
            "steps" => Dataset::Steps,
            "all" => Dataset::All,
            _ => return None,
        })
    }

    /// The i18n key of the request-panel RU text (also the message fallback text).
    pub fn panel_key(self) -> &'static str {
        match self {
            Dataset::Body => "curator.request_body",
            Dataset::Food => "curator.request_food",
            Dataset::Weight => "curator.request_weight",
            Dataset::Steps => "curator.request_steps",
            Dataset::All => "curator.request_all",
        }
    }

    /// The i18n key of the short confirmation label used in the data_share `text`.
    pub fn shared_key(self) -> &'static str {
        match self {
            Dataset::Body => "curator.shared_body",
            Dataset::Food => "curator.shared_food",
            Dataset::Weight => "curator.shared_weight",
            Dataset::Steps => "curator.shared_steps",
            Dataset::All => "curator.shared_all",
        }
    }
}

// ── Per-dataset builders (real stores only) ──

async fn build_body() -> Value {
    let latest_kg = local::list_weight_entries().await.last().map(|e| e.weight_kg);
    let sex = profile::get_sex().map(|s| match s {
        profile::Sex::Male => "male",
        profile::Sex::Female => "female",
    });
    json!({
        "weight_kg": latest_kg,
        "height_cm": profile::get_height_cm(),
        "birth_year": profile::get_birth_year(),
        "sex": sex,
    })
}

async fn build_weight() -> Value {
    let entries = local::list_weight_entries().await;
    let series: Vec<Value> = entries
        .iter()
        .map(|e| json!({ "date": e.date, "kg": e.weight_kg }))
        .collect();

    let trend = weight_trend::weight_trend(&entries, DEFAULT_WINDOW_DAYS);
    let balance = match trend.balance() {
        BalanceState::Deficit => "deficit",
        BalanceState::Surplus => "surplus",
        BalanceState::Maintenance => "maintenance",
    };
    // Slope / confidence / direction / days come from the trend estimate; each is
    // null when the window can't support it (honest, not fabricated).
    let (slope, confidence, direction, days) = match trend {
        WeightTrend::Insufficient { days } => (None, None, None, days),
        WeightTrend::Tentative { direction, slope_kg_per_week, days } => {
            (Some(slope_kg_per_week), None, Some(dir_str(direction)), days)
        }
        WeightTrend::Estimated { direction, slope_kg_per_week, confidence, days } => (
            Some(slope_kg_per_week),
            Some(confidence),
            Some(dir_str(direction)),
            days,
        ),
    };
    json!({
        "series": series,
        "balance": balance,
        "slope_kg_per_week": slope,
        "confidence": confidence,
        "direction": direction,
        "days": days,
    })
}

fn dir_str(d: Direction) -> &'static str {
    match d {
        Direction::Down => "down",
        Direction::Up => "up",
    }
}

async fn build_steps() -> Value {
    let series: Vec<Value> = local::list_step_entries()
        .await
        .iter()
        .map(|e| json!({ "date": e.date, "steps": e.steps }))
        .collect();
    json!({ "series": series })
}

async fn build_food() -> Value {
    let foods: std::collections::BTreeMap<String, api_types::Food> =
        local::list_foods().await.into_iter().map(|f| (f.id.clone(), f)).collect();

    // Last 7 calendar days, newest first.
    let today = crate::services::local::today_date();
    let mut days: Vec<Value> = Vec::new();
    for i in 0..7 {
        let date = (today - chrono::Duration::days(i)).format("%Y-%m-%d").to_string();
        let diary = local::list_diary(&date).await;
        if diary.is_empty() {
            continue;
        }

        let mut entries: Vec<Value> = Vec::new();
        let (mut tk, mut tp, mut tf, mut tc) = (0.0, 0.0, 0.0, 0.0);
        for e in &diary {
            let Some(food) = foods.get(&e.food_id) else { continue };
            let eaten = (e.grams - e.waste_grams).max(0.0);
            let factor = eaten / 100.0;
            let kcal = food.effective_kcal() * factor;
            let protein = food.protein * factor;
            let fat = food.fat * factor;
            let carbs = food.carbs * factor;
            tk += kcal;
            tp += protein;
            tf += fat;
            tc += carbs;
            entries.push(json!({
                "name": food.name,
                "grams": eaten,
                "kcal": kcal,
                "protein": protein,
                "fat": fat,
                "carbs": carbs,
            }));
        }

        days.push(json!({
            "date": date,
            "entries": entries,
            "totals": { "kcal": tk, "protein": tp, "fat": tf, "carbs": tc },
        }));
    }
    json!({ "days": days })
}

/// Gather `dataset` into its typed data_share envelope value.
///
/// The envelope is ALWAYS an object keyed by dataset name — a single dataset is
/// `{"weight": {...}}`, "all" is the 4-key map. Keying single shares too keeps the
/// reader (admin `datasets_from_payload`) uniform and unambiguous: a bare
/// `{"series": …}` couldn't be told apart (weight vs steps both carry `series`).
pub async fn build(dataset: Dataset) -> Value {
    match dataset {
        Dataset::Body => json!({ "body": build_body().await }),
        Dataset::Weight => json!({ "weight": build_weight().await }),
        Dataset::Steps => json!({ "steps": build_steps().await }),
        Dataset::Food => json!({ "food": build_food().await }),
        Dataset::All => json!({
            "body": build_body().await,
            "weight": build_weight().await,
            "steps": build_steps().await,
            "food": build_food().await,
        }),
    }
}

/// Build the data_share message the user sends on "Поделиться": the payload JSON
/// STRING plus the short RU confirmation `text`. FAIL LOUDLY on a serialize error.
pub async fn share_message(dataset: Dataset) -> Result<(String, String), String> {
    let value = build(dataset).await;
    let payload = serde_json::to_string(&value).map_err(|e| format!("serialize error: {e}"))?;
    let text = i18n::t(dataset.shared_key()).to_string();
    Ok((text, payload))
}

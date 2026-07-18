use std::collections::BTreeMap;

use api_types::*;
use arti_pipes::executor::PromptExecutor;
use arti_pipes::llm_executors::qwen::Qwen;
use futures::StreamExt;
use schemars::JsonSchema;
use serde::Deserialize;

#[derive(Debug, Deserialize, JsonSchema)]
struct NutritionResponse {
    kcal: NutrientDetail,
    protein: NutrientDetail,
    fat: NutrientDetail,
    carbs: NutrientDetail,
    custom_nutrients: BTreeMap<String, NutrientDetail>,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct NutrientDetail {
    min_value: ValueUnit,
    max_value: ValueUnit,
    recommended: ValueUnit,
    /// Why this value is appropriate for this food
    comment: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct ValueUnit {
    value: f64,
    /// One of: kcal, kg, g, mg, mkg
    unit: String,
}

impl NutrientDetail {
    fn into_api(self) -> AiNutrientDetail {
        AiNutrientDetail {
            min_value: AiValueWithUnit { value: self.min_value.value, unit: self.min_value.unit },
            max_value: AiValueWithUnit { value: self.max_value.value, unit: self.max_value.unit },
            recommended: AiValueWithUnit { value: self.recommended.value, unit: self.recommended.unit },
            comment: self.comment,
        }
    }
}

pub async fn lookup(executor: &Qwen, input: AiLookupInput) -> Result<AiLookupOutput, ApiError> {
    let custom_part = if input.custom_nutrients.is_empty() {
        String::new()
    } else {
        let keys: Vec<String> = input
            .custom_nutrients
            .iter()
            .map(|s| format!("\"{}\"", s.key))
            .collect();
        let descriptions: Vec<String> = input
            .custom_nutrients
            .iter()
            .map(|s| format!("{} = {}", s.key, s.name))
            .collect();
        format!(
            "\n\nAlso provide values for these custom nutrients in custom_nutrients map. \
             Use ONLY these strings as keys: {}. \
             Reference: {}.",
            keys.join(", "),
            descriptions.join(", "),
        )
    };

    // When the name came from a food PHOTO the grams are the cooked/as-served
    // portion, so per-100 g values must be for the cooked food (not raw/dry) —
    // else a cooked weight × raw density over-counts calories ~2–3×.
    let form_part = if input.as_served {
        " This food was photographed on a plate, ready to eat: give values for the \
         COOKED / as-served state (e.g. boiled pasta ~130 kcal/100 g, not dry ~350), \
         even if the name sounds like a raw ingredient.\n\n"
    } else {
        "\n\n"
    };
    let prompt = format!(
        "You are a nutritional database. For the food item \"{name}\", provide nutritional \
         values per 100 grams.{form}\
         For each nutrient (kcal, protein, fat, carbs{custom}), provide:\n\
         - min_value: lowest reasonable value for this food\n\
         - max_value: highest reasonable value for this food\n\
         - recommended: the most likely value to select\n\
         - comment: brief explanation why this value is appropriate\n\n\
         Use these units: kcal for calories, g/mg/mkg/kg for weights.\n\
         All values are per 100g of the product.",
        name = input.name,
        custom = custom_part,
        form = form_part,
    );

    tracing::info!("AI lookup for: {}", input.name);

    let result = executor
        .execute::<NutritionResponse>(prompt)
        .await
        .map_err(|e| {
            tracing::error!("LLM execute error: {e:?}");
            ApiError::InternalError
        })?;

    let mut thinking_stream = result.thinking_stream;
    tokio::spawn(async move {
        while let Some(token) = thinking_stream.next().await {
            if let Ok(t) = token {
                eprint!("{}", t.content);
            }
        }
        eprintln!();
    });

    let mut content_stream = result.content_stream;
    tokio::spawn(async move {
        while let Some(token) = content_stream.next().await {
            if let Ok(t) = token {
                eprint!("{}", t.content);
            }
        }
        eprintln!();
    });

    let output = result.output.await.map_err(|e| {
        tracing::error!("LLM output error: {e:?}");
        ApiError::InternalError
    })?;

    tracing::info!("AI lookup complete: {}", output.result);

    let response: NutritionResponse = serde_json::from_str(&output.result).map_err(|e| {
        tracing::error!("LLM response parse error: {e}, raw: {}", output.result);
        ApiError::InternalError
    })?;

    let key_to_name: BTreeMap<String, String> = input
        .custom_nutrients
        .iter()
        .map(|s| (s.key.clone(), s.name.clone()))
        .collect();

    let nutrients: BTreeMap<String, AiNutrientDetail> = response
        .custom_nutrients
        .into_iter()
        .map(|(ai_key, v)| {
            let display_name = key_to_name
                .get(&ai_key)
                .cloned()
                .unwrap_or(ai_key);
            (display_name, v.into_api())
        })
        .collect();

    Ok(AiLookupOutput {
        name: None,
        kcal: response.kcal.into_api(),
        protein: response.protein.into_api(),
        fat: response.fat.into_api(),
        carbs: response.carbs.into_api(),
        nutrients,
        package_weight: None,
    })
}

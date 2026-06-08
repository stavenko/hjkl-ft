use std::collections::BTreeMap;

use api_types::*;
use serde::{Deserialize, Serialize};

use crate::config::VisionConfig;

#[derive(Debug, Serialize)]
struct VisionRequest {
    model: String,
    messages: Vec<VisionMessage>,
    response_format: ResponseFormat,
    stream: bool,
}

#[derive(Debug, Serialize)]
struct ResponseFormat {
    r#type: String,
    json_schema: JsonSchemaField,
}

#[derive(Debug, Serialize)]
struct JsonSchemaField {
    name: String,
    schema: serde_json::Value,
    strict: bool,
}

#[derive(Debug, Serialize)]
struct VisionMessage {
    role: String,
    content: Vec<ContentPart>,
}

#[derive(Debug, Serialize)]
#[serde(tag = "type")]
enum ContentPart {
    #[serde(rename = "text")]
    Text { text: String },
    #[serde(rename = "image_url")]
    ImageUrl { image_url: ImageUrl },
}

#[derive(Debug, Serialize)]
struct ImageUrl {
    url: String,
}

#[derive(Debug, Deserialize)]
struct VisionResponse {
    choices: Vec<VisionChoice>,
}

#[derive(Debug, Deserialize)]
struct VisionChoice {
    message: VisionMsg,
}

#[derive(Debug, Deserialize)]
struct VisionMsg {
    content: String,
}

#[derive(Debug, Deserialize)]
struct LabelData {
    product_name: String,
    energy: LabelValue,
    protein: LabelValue,
    fat: LabelValue,
    carbs: LabelValue,
    package_weight: Option<LabelValue>,
}

#[derive(Debug, Deserialize)]
struct LabelValue {
    value: f64,
    unit: String,
}

/// Step 1: Vision model reads the label — name, KBJU, package_weight only.
/// No custom nutrients — those are handled by a separate text LLM call.
pub async fn read_label(
    config: &VisionConfig,
    images: &[String],
) -> Result<AiLookupOutput, ApiError> {
    let prompt =
        "Look at the nutrition label(s) in these images. Extract ONLY:\n\
         - product_name\n\
         - energy: value and unit EXACTLY as on the label (\"kcal\", \"kJ\", \"кДж\", \"ккал\"). Do NOT convert.\n\
         - protein: value and unit (\"g\", \"г\")\n\
         - fat: value and unit (\"g\", \"г\")\n\
         - carbs: value and unit (\"g\", \"г\")\n\
         All nutrition values must be per 100g.\n\
         - package_weight: the weight of the EDIBLE product only, with value and unit (\"g\", \"kg\", \"ml\", \"l\", \"кг\", \"г\", \"мл\", \"л\"). \
         For products in brine/marinade/syrup/oil, use the DRAINED weight. \
         If not found, return null.\n\n\
         If values on the label are per serving, convert to per 100g.\n\
         Do NOT try to estimate any nutrients beyond what is on the label."
            .to_string();

    let mut content = vec![ContentPart::Text { text: prompt }];
    for img_base64 in images {
        let raw_b64 = if img_base64.starts_with("data:") {
            img_base64.split(',').nth(1).unwrap_or(img_base64).to_string()
        } else {
            img_base64.clone()
        };

        let jpeg_b64 = crate::providers::image_convert::ensure_jpeg_base64(&raw_b64)
            .map_err(|e| {
                tracing::error!("Image conversion error: {e}");
                ApiError::BadRequest(format!("Image conversion failed: {e}"))
            })?;

        content.push(ContentPart::ImageUrl {
            image_url: ImageUrl {
                url: format!("data:image/jpeg;base64,{jpeg_b64}"),
            },
        });
    }

    let value_unit_schema = serde_json::json!({
        "type": "object",
        "properties": {
            "value": { "type": "number" },
            "unit": { "type": "string" }
        },
        "required": ["value", "unit"],
        "additionalProperties": false
    });

    let schema = serde_json::json!({
        "type": "object",
        "properties": {
            "product_name": { "type": "string" },
            "energy": value_unit_schema,
            "protein": value_unit_schema,
            "fat": value_unit_schema,
            "carbs": value_unit_schema,
            "package_weight": {
                "oneOf": [
                    { "type": "null" },
                    value_unit_schema
                ]
            }
        },
        "required": ["product_name", "energy", "protein", "fat", "carbs", "package_weight"],
        "additionalProperties": false
    });

    let request_body = VisionRequest {
        model: config.model.clone(),
        messages: vec![VisionMessage {
            role: "user".into(),
            content,
        }],
        response_format: ResponseFormat {
            r#type: "json_schema".into(),
            json_schema: JsonSchemaField {
                name: "label".into(),
                schema,
                strict: true,
            },
        },
        stream: false,
    };

    let client = reqwest::Client::new();
    let url = format!("{}/chat/completions", config.api_base.trim_end_matches('/'));

    let mut req = client.post(&url).json(&request_body);
    if let Some(ref key) = config.api_key {
        if !key.is_empty() {
            req = req.header("Authorization", format!("Bearer {key}"));
        }
    }

    tracing::info!("Vision request to {} with {} image(s)", config.model, images.len());

    let resp = req.send().await.map_err(|e| {
        tracing::error!("Vision HTTP error: {e}");
        ApiError::InternalError
    })?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        tracing::error!("Vision API error {status}: {body}");
        return Err(ApiError::InternalError);
    }

    let vision_resp = resp.json::<VisionResponse>().await.map_err(|e| {
        tracing::error!("Vision response parse error: {e}");
        ApiError::InternalError
    })?;

    let content_str = &vision_resp.choices[0].message.content;
    tracing::info!("Vision label result: {content_str}");

    let parsed: LabelData = serde_json::from_str(content_str).map_err(|e| {
        tracing::error!("Vision JSON parse error: {e}, raw: {content_str}");
        ApiError::InternalError
    })?;

    let kcal = convert_energy(&parsed.energy);
    let protein = convert_mass(&parsed.protein);
    let fat = convert_mass(&parsed.fat);
    let carbs = convert_mass(&parsed.carbs);
    let package_weight = parsed.package_weight.map(|pw| convert_weight(&pw));

    tracing::info!(
        "Converted: {kcal:.1} kcal, P {protein:.1}g, F {fat:.1}g, C {carbs:.1}g, pkg {package_weight:?}g"
    );

    let exact = |v: f64, unit: &str| AiNutrientDetail {
        min_value: AiValueWithUnit { value: v, unit: unit.to_string() },
        max_value: AiValueWithUnit { value: v, unit: unit.to_string() },
        recommended: AiValueWithUnit { value: v, unit: unit.to_string() },
        comment: "Read from label".to_string(),
    };

    Ok(AiLookupOutput {
        name: Some(parsed.product_name),
        kcal: exact(kcal, "kcal"),
        protein: exact(protein, "g"),
        fat: exact(fat, "g"),
        carbs: exact(carbs, "g"),
        nutrients: BTreeMap::new(),
        package_weight,
    })
}

/// Convert energy to kcal. 1 kJ ≈ 0.239 kcal.
fn convert_energy(v: &LabelValue) -> f64 {
    match v.unit.to_lowercase().as_str() {
        "kj" | "кдж" => v.value * 0.239,
        _ => v.value, // "kcal", "ккал"
    }
}

/// Convert nutrition mass to grams.
fn convert_mass(v: &LabelValue) -> f64 {
    match v.unit.to_lowercase().as_str() {
        "mg" | "мг" => v.value / 1000.0,
        _ => v.value, // "g", "г"
    }
}

/// Convert package weight to grams.
fn convert_weight(v: &LabelValue) -> f64 {
    match v.unit.to_lowercase().as_str() {
        "kg" | "кг" => v.value * 1000.0,
        "l" | "л" => v.value * 1000.0,
        "ml" | "мл" => v.value,
        _ => v.value, // "g", "г"
    }
}

use std::collections::BTreeMap;

use api_types::*;
use arti_pipes::executor::PromptExecutor;
use arti_pipes::llm_executors::qwen::Qwen;
use futures::StreamExt;
use schemars::JsonSchema;
use serde::Deserialize;
use wasm_bindgen::JsCast;
use wasm_bindgen::JsValue;
use wasm_bindgen_futures::JsFuture;

use super::{auth, config};

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

fn strip_code_fences(s: &str) -> &str {
    let s = s.trim();
    if let Some(rest) = s.strip_prefix("```") {
        let rest = rest.trim_start_matches(|c: char| c.is_alphanumeric());
        let rest = rest.trim_start_matches('\n');
        rest.strip_suffix("```").unwrap_or(rest).trim()
    } else {
        s
    }
}

fn unwrap_schema_envelope(s: &str) -> &str {
    const PREFIX: &str = r#""properties":"#;
    if let Some(idx) = s.find(PREFIX) {
        let start = idx + PREFIX.len();
        if let Some(obj_start) = s[start..].find('{') {
            let inner_start = start + obj_start;
            let mut depth = 0i32;
            let mut end = inner_start;
            for (i, c) in s[inner_start..].char_indices() {
                match c {
                    '{' => depth += 1,
                    '}' => {
                        depth -= 1;
                        if depth == 0 {
                            end = inner_start + i + 1;
                            break;
                        }
                    }
                    _ => {}
                }
            }
            if depth == 0 {
                return &s[inner_start..end];
            }
        }
    }
    s
}

fn build_executor() -> Result<Qwen, String> {
    let cfg = config::get();
    let token = auth::get_token().ok_or_else(|| "not authenticated".to_string())?;
    let executor = Qwen::builder()
        .api_base(&cfg.ai_base_url)
        .api_key(token)
        .model("@cf/zai-org/glm-4.7-flash")
        .build();
    Ok(executor)
}

pub async fn lookup(input: &AiLookupInput) -> Result<AiLookupOutput, String> {
    let executor = build_executor()?;

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

    let prompt = format!(
        "You are a nutritional database. For the food item \"{name}\", provide nutritional \
         values per 100 grams.\n\n\
         For each nutrient (kcal, protein, fat, carbs{custom}), provide:\n\
         - min_value: lowest reasonable value for this food\n\
         - max_value: highest reasonable value for this food\n\
         - recommended: the most likely value to select\n\
         - comment: brief explanation why this value is appropriate\n\n\
         Use these units: kcal for calories, g/mg/mkg/kg for weights.\n\
         All values are per 100g of the product.",
        name = input.name,
        custom = custom_part,
    );

    let result = executor
        .execute::<NutritionResponse>(prompt)
        .await
        .map_err(|e| format!("LLM execute error: {e:?}"))?;

    let mut thinking_stream = result.thinking_stream;
    wasm_bindgen_futures::spawn_local(async move {
        while let Some(token) = thinking_stream.next().await {
            if let Ok(t) = token {
                leptos::logging::log!("[think] {}", t.content);
            }
        }
    });

    let mut content_stream = result.content_stream;
    wasm_bindgen_futures::spawn_local(async move {
        while let Some(token) = content_stream.next().await {
            if let Ok(t) = token {
                leptos::logging::log!("[content] {}", t.content);
            }
        }
    });

    let output = result.output.await.map_err(|e| format!("LLM output error: {e:?}"))?;

    let raw = output.result.trim();
    let json_str = strip_code_fences(raw);

    let response: NutritionResponse = serde_json::from_str(json_str)
        .or_else(|_| {
            let unwrapped = unwrap_schema_envelope(json_str);
            serde_json::from_str(unwrapped)
        })
        .map_err(|e| format!("parse error: {e}, raw: {raw}"))?;

    let key_to_name: BTreeMap<String, String> = input
        .custom_nutrients
        .iter()
        .map(|s| (s.key.clone(), s.name.clone()))
        .collect();

    let nutrients: BTreeMap<String, AiNutrientDetail> = response
        .custom_nutrients
        .into_iter()
        .filter_map(|(ai_key, v)| {
            let display_name = key_to_name.get(&ai_key)?;
            Some((display_name.clone(), v.into_api()))
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

pub async fn vision(input: &AiVisionInput) -> Result<AiLookupOutput, String> {
    let base = &config::get().ai_base_url;
    let url = format!("{base}/food/ai-vision");
    let token = auth::get_token().ok_or_else(|| "not authenticated".to_string())?;
    let body_str = serde_json::to_string(input).map_err(|e| e.to_string())?;

    let opts = web_sys::RequestInit::new();
    opts.set_method("POST");
    opts.set_body(&JsValue::from_str(&body_str));

    let headers = web_sys::Headers::new().map_err(|e| format!("{e:?}"))?;
    headers.set("Content-Type", "application/json").map_err(|e| format!("{e:?}"))?;
    headers.set("Authorization", &format!("Bearer {token}")).map_err(|e| format!("{e:?}"))?;
    opts.set_headers(&headers);

    let request = web_sys::Request::new_with_str_and_init(&url, &opts)
        .map_err(|e| format!("{e:?}"))?;

    let window = web_sys::window().expect("no window");
    let resp_val = JsFuture::from(window.fetch_with_request(&request))
        .await
        .map_err(|e| format!("{e:?}"))?;
    let resp: web_sys::Response = resp_val.dyn_into().map_err(|_| "not a Response".to_string())?;

    let text = JsFuture::from(resp.text().map_err(|e| format!("{e:?}"))?)
        .await
        .map_err(|e| format!("{e:?}"))?;
    let text = text.as_string().ok_or("response not string")?;

    if !resp.ok() {
        return Err(format!("HTTP {}: {}", resp.status(), text));
    }

    serde_json::from_str(&text).map_err(|e| format!("parse error: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn print_schema() {
        let schema = schemars::schema_for!(NutritionResponse);
        println!("{}", serde_json::to_string_pretty(&schema).unwrap());
    }
}

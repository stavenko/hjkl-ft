use actix_web::web;
use api_types::*;
use arti_pipes::llm_executors::qwen::Qwen;

use crate::api::postcard_body::Postcard;
use crate::api::response::ApiResponse;
use crate::config::VisionConfig;
use crate::use_cases;

pub async fn ai_vision(
    vision_config: web::Data<VisionConfig>,
    llm_executor: web::Data<Qwen>,
    body: Postcard<AiVisionInput>,
) -> ApiResponse<AiLookupOutput> {
    let input = body.into_inner();
    let custom_nutrients = input.custom_nutrients.clone();

    let mut result = match use_cases::ai_vision::read_label(&vision_config, &input.images).await {
        Ok(r) => r,
        Err(e) => return ApiResponse(Err(e)),
    };

    if !custom_nutrients.is_empty() {
        let product_name = result.name.clone().unwrap_or_default();
        if !product_name.is_empty() {
            let lookup_input = AiLookupInput {
                name: product_name,
                custom_nutrients,
            };
            match use_cases::ai_lookup::lookup(&llm_executor, lookup_input).await {
                Ok(nutrient_result) => {
                    result.nutrients = nutrient_result.nutrients;
                }
                Err(e) => {
                    tracing::warn!("Custom nutrient lookup failed, returning without them: {e:?}");
                }
            }
        }
    }

    ApiResponse(Ok(result))
}

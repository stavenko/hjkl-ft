use actix_web::web;
use api_types::*;
use arti_pipes::llm_executors::qwen::Qwen;

use crate::api::postcard_body::Postcard;
use crate::api::response::ApiResponse;
use crate::use_cases;

pub async fn ai_lookup(
    executor: web::Data<Qwen>,
    body: Postcard<AiLookupInput>,
) -> ApiResponse<AiLookupOutput> {
    let result = use_cases::ai_lookup::lookup(&executor, body.into_inner()).await;
    ApiResponse(result)
}

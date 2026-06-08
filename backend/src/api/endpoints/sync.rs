use actix_web::web;
use api_types::*;

use crate::api::postcard_body::Postcard;
use crate::api::response::ApiResponse;
use crate::providers::database::Database;
use crate::use_cases;

pub async fn sync_dump(
    db: web::Data<Database>,
) -> ApiResponse<SyncDumpResponse> {
    let result = use_cases::sync::dump(&db);
    ApiResponse(result)
}

pub async fn sync_push(
    db: web::Data<Database>,
    body: Postcard<SyncPushPayload>,
) -> ApiResponse<SyncPushResponse> {
    let result = use_cases::sync::push(&db, body.into_inner());
    ApiResponse(result)
}

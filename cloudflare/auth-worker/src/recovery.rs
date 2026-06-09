use serde_json::json;
use worker::*;

use crate::token;
use crate::types::{ErrorResponse, RecoveryAuthRequest, RecoverySetRequest};
use crate::user_do::create_recovery_hash;

pub async fn set_recovery_key(mut req: Request, ctx: RouteContext<()>) -> Result<Response> {
    // Validate session token
    let user_id = match token::validate_from_header(&req, &ctx.env) {
        Ok(id) => id,
        Err(e) => {
            let body = ErrorResponse {
                error: format!("authentication failed: {e}"),
            };
            return Ok(Response::from_json(&body)?.with_status(401));
        }
    };

    // Parse request body
    let body: RecoverySetRequest = req
        .json()
        .await
        .map_err(|e| Error::RustError(format!("invalid request body: {e}")))?;

    // Hash the recovery key
    let hash_data = create_recovery_hash(&body.recovery_key);
    let hash_json = serde_json::to_string(&hash_data)
        .map_err(|e| Error::RustError(format!("serialize hash: {e}")))?;

    // Get UserDO stub by user_id and store the hash via internal request
    let namespace = ctx.env.durable_object("USER_DO")?;
    let stub = namespace.id_from_name(&user_id)?.get_stub()?;

    // Send internal request to UserDO to store the recovery hash
    let internal_req = Request::new_with_init(
        "https://internal/recovery/set",
        RequestInit::new()
            .with_method(Method::Post)
            .with_body(Some(wasm_bindgen::JsValue::from_str(&hash_json))),
    )?;

    let resp = stub.fetch_with_request(internal_req).await?;
    if resp.status_code() != 200 {
        return Response::error("failed to store recovery hash", 500);
    }

    Response::from_json(&json!({ "status": "ok" }))
}

pub async fn authenticate_with_recovery(
    mut req: Request,
    ctx: RouteContext<()>,
) -> Result<Response> {
    // Parse request body
    let body: RecoveryAuthRequest = req
        .json()
        .await
        .map_err(|e| Error::RustError(format!("invalid request body: {e}")))?;

    // Get UserDO stub by username
    let namespace = ctx.env.durable_object("USER_DO")?;
    let stub = namespace.id_from_name(&body.username)?.get_stub()?;

    // Send internal request to UserDO to verify recovery key
    let verify_payload = serde_json::to_string(&json!({ "recovery_key": body.recovery_key }))
        .map_err(|e| Error::RustError(format!("serialize: {e}")))?;

    let internal_req = Request::new_with_init(
        "https://internal/recovery/verify",
        RequestInit::new()
            .with_method(Method::Post)
            .with_body(Some(wasm_bindgen::JsValue::from_str(&verify_payload))),
    )?;

    let mut resp = stub.fetch_with_request(internal_req).await?;
    if resp.status_code() != 200 {
        let body = ErrorResponse {
            error: "invalid recovery key".into(),
        };
        return Ok(Response::from_json(&body)?.with_status(401));
    }

    // Parse verification result
    let result: serde_json::Value = resp.json().await?;
    let valid = result
        .get("valid")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    if !valid {
        let body = ErrorResponse {
            error: "invalid recovery key".into(),
        };
        return Ok(Response::from_json(&body)?.with_status(401));
    }

    // Issue a new session token
    let secret = ctx
        .env
        .secret("JWT_SECRET")
        .map(|s| s.to_string())
        .map_err(|_| Error::RustError("JWT_SECRET not configured".into()))?;

    let token_response =
        token::create_token(&body.username, vec!["auth".to_string()], &secret)?;

    Response::from_json(&token_response)
}

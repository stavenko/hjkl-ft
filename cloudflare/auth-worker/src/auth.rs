use worker::*;

use crate::token;
use crate::types::ErrorResponse;
use crate::{auth_do_stub, do_request};

// ---- Registration ----

pub async fn register_begin(mut req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let origin = crate::request_origin(&req);
    let body: serde_json::Value = req.json().await.unwrap_or_default();
    let display_name = body
        .get("display_name")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let stub = auth_do_stub(&ctx.env)?;
    let internal_req = do_request("/register/begin", &serde_json::json!({
        "display_name": display_name,
        "origin": origin
    }))?;
    let resp = stub.fetch_with_request(internal_req).await?;
    Ok(resp)
}

pub async fn register_finish(mut req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let origin = crate::request_origin(&req);
    let body: serde_json::Value = req
        .json()
        .await
        .map_err(|e| Error::RustError(format!("invalid request body: {e}")))?;

    let credential = body
        .get("credential")
        .cloned()
        .ok_or_else(|| Error::RustError("missing credential".into()))?;
    let user_id = body
        .get("user_id")
        .and_then(|v| v.as_str())
        .ok_or_else(|| Error::RustError("missing user_id in finish request".into()))?;
    let fingerprint = body
        .get("fingerprint")
        .and_then(|v| v.as_str())
        .unwrap_or("");

    let stub = auth_do_stub(&ctx.env)?;
    let do_body = serde_json::json!({ "credential": credential, "user_id": user_id, "origin": origin });
    let internal_req = do_request("/register/finish", &do_body)?;
    let resp = stub.fetch_with_request(internal_req).await?;

    if resp.status_code() != 200 {
        return Ok(resp);
    }

    // Issue JWT after successful registration
    let secret = token::jwt_secret(&ctx.env).await?;

    let (token_response, token_id) =
        token::create_token(user_id, fingerprint, vec!["auth".to_string()], &secret)?;

    // Store token metadata in DO
    token::store_token_in_do(&ctx.env, &token_id, user_id, fingerprint).await?;

    Response::from_json(&serde_json::json!({
        "ok": true,
        "user_id": user_id,
        "token": token_response.token,
        "expires_at": token_response.expires_at,
    }))
}

// ---- Authentication (discoverable, no username) ----

pub async fn authenticate_begin(req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let origin = crate::request_origin(&req);
    let stub = auth_do_stub(&ctx.env)?;
    let internal_req = do_request("/authenticate/begin", &serde_json::json!({ "origin": origin }))?;
    let resp = stub.fetch_with_request(internal_req).await?;
    Ok(resp)
}

pub async fn authenticate_finish(mut req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let origin = crate::request_origin(&req);
    let body: serde_json::Value = req
        .json()
        .await
        .map_err(|e| Error::RustError(format!("invalid request body: {e}")))?;

    let credential = body
        .get("credential")
        .cloned()
        .ok_or_else(|| Error::RustError("missing credential".into()))?;
    let fingerprint = body
        .get("fingerprint")
        .and_then(|v| v.as_str())
        .unwrap_or("");

    let stub = auth_do_stub(&ctx.env)?;
    let do_body = serde_json::json!({ "credential": credential, "origin": origin });
    let internal_req = do_request("/authenticate/finish", &do_body)?;
    let mut resp = stub.fetch_with_request(internal_req).await?;

    if resp.status_code() != 200 {
        return Ok(resp);
    }

    // Parse the DO response to get user_id, then issue a JWT
    let do_result: serde_json::Value = resp
        .json()
        .await
        .map_err(|e| Error::RustError(format!("parse DO response: {e}")))?;

    let user_id = do_result
        .get("user_id")
        .and_then(|v| v.as_str())
        .ok_or_else(|| Error::RustError("DO did not return user_id".into()))?;

    let secret = token::jwt_secret(&ctx.env).await?;

    let (token_response, token_id) =
        token::create_token(user_id, fingerprint, vec!["auth".to_string()], &secret)?;

    // Store token metadata in DO
    token::store_token_in_do(&ctx.env, &token_id, user_id, fingerprint).await?;

    Response::from_json(&serde_json::json!({
        "user_id": user_id,
        "token": token_response.token,
        "expires_at": token_response.expires_at,
    }))
}

// ---- Add device (requires existing session) ----

pub async fn add_device_begin(req: Request, ctx: RouteContext<()>) -> Result<Response> {
    // Validate existing session token
    let user_id = match token::validate_from_header(&req, &ctx.env).await {
        Ok(sub) => sub,
        Err(e) => {
            let body = ErrorResponse {
                error: format!("authentication required: {e}"),
            };
            return Ok(Response::from_json(&body)?.with_status(401));
        }
    };

    let origin = crate::request_origin(&req);
    let stub = auth_do_stub(&ctx.env)?;
    let do_body = serde_json::json!({ "user_id": user_id, "origin": origin });
    let internal_req = do_request("/register/begin", &do_body)?;
    let resp = stub.fetch_with_request(internal_req).await?;

    Ok(resp)
}

pub async fn add_device_finish(mut req: Request, ctx: RouteContext<()>) -> Result<Response> {
    // Validate existing session token
    let user_id = match token::validate_from_header(&req, &ctx.env).await {
        Ok(sub) => sub,
        Err(e) => {
            let body = ErrorResponse {
                error: format!("authentication required: {e}"),
            };
            return Ok(Response::from_json(&body)?.with_status(401));
        }
    };

    let origin = crate::request_origin(&req);
    let body: serde_json::Value = req
        .json()
        .await
        .map_err(|e| Error::RustError(format!("invalid request body: {e}")))?;

    let credential = body
        .get("credential")
        .cloned()
        .ok_or_else(|| Error::RustError("missing credential".into()))?;

    let stub = auth_do_stub(&ctx.env)?;
    let do_body = serde_json::json!({ "credential": credential, "user_id": user_id, "origin": origin });
    let internal_req = do_request("/register/finish", &do_body)?;
    let resp = stub.fetch_with_request(internal_req).await?;

    Ok(resp)
}

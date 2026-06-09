use worker::*;

use crate::token;
use crate::types::{
    AuthenticateBeginRequest, AuthenticateFinishRequest, ErrorResponse, RegisterBeginRequest,
    RegisterFinishRequest,
};

/// Build an internal POST request to the Durable Object with the given path and JSON body.
fn do_request(path: &str, body: &serde_json::Value) -> Result<Request> {
    let url = format!("https://internal{path}");
    let body_str = serde_json::to_string(body)
        .map_err(|e| Error::RustError(format!("serialize DO request: {e}")))?;
    Request::new_with_init(
        &url,
        RequestInit::new()
            .with_method(Method::Post)
            .with_body(Some(wasm_bindgen::JsValue::from_str(&body_str))),
    )
}

/// Get a DO stub for a given username.
fn user_stub(ctx: &RouteContext<()>, username: &str) -> Result<worker::durable::Stub> {
    let namespace = ctx.env.durable_object("USER_DO")?;
    namespace.id_from_name(username)?.get_stub()
}

// ---- Registration ----

pub async fn register_begin(mut req: Request, ctx: RouteContext<()>) -> Result<Response> {
    // Username is optional — generate UUID if not provided
    let username = match req.json::<RegisterBeginRequest>().await {
        Ok(body) if !body.username.is_empty() => body.username,
        _ => uuid::Uuid::new_v4().to_string(),
    };

    let stub = user_stub(&ctx, &username)?;

    let do_body = serde_json::json!({ "username": username });
    let internal_req = do_request("/passkey/register/begin", &do_body)?;
    let resp = stub.fetch_with_request(internal_req).await?;

    Ok(resp)
}

pub async fn register_finish(mut req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let body: serde_json::Value = req
        .json()
        .await
        .map_err(|e| Error::RustError(format!("invalid request body: {e}")))?;

    let username = body.get("username")
        .and_then(|v| v.as_str())
        .ok_or_else(|| Error::RustError("missing username in finish request".into()))?;
    let credential = body.get("credential")
        .cloned()
        .ok_or_else(|| Error::RustError("missing credential".into()))?;

    let stub = user_stub(&ctx, username)?;

    let do_body = serde_json::json!({ "credential": credential });
    let internal_req = do_request("/passkey/register/finish", &do_body)?;
    let resp = stub.fetch_with_request(internal_req).await?;

    if resp.status_code() != 200 {
        return Ok(resp);
    }

    // Issue JWT after successful registration
    let secret = ctx
        .env
        .secret("JWT_SECRET")
        .map(|s| s.to_string())
        .map_err(|_| Error::RustError("JWT_SECRET not configured".into()))?;

    let token_response = token::create_token(username, vec!["auth".to_string()], &secret)?;

    Response::from_json(&serde_json::json!({
        "ok": true,
        "user_id": username,
        "token": token_response.token,
        "expires_at": token_response.expires_at,
    }))
}

// ---- Authentication ----

pub async fn authenticate_begin(mut req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let body: AuthenticateBeginRequest = req
        .json()
        .await
        .map_err(|e| Error::RustError(format!("invalid request body: {e}")))?;

    let stub = user_stub(&ctx, &body.username)?;

    let internal_req = do_request("/passkey/authenticate/begin", &serde_json::json!({}))?;
    let resp = stub.fetch_with_request(internal_req).await?;

    Ok(resp)
}

pub async fn authenticate_finish(mut req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let body: AuthenticateFinishRequest = req
        .json()
        .await
        .map_err(|e| Error::RustError(format!("invalid request body: {e}")))?;

    let stub = user_stub(&ctx, &body.username)?;

    let do_body = serde_json::json!({ "credential": body.credential });
    let internal_req = do_request("/passkey/authenticate/finish", &do_body)?;
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

    let secret = ctx
        .env
        .secret("JWT_SECRET")
        .map(|s| s.to_string())
        .map_err(|_| Error::RustError("JWT_SECRET not configured".into()))?;

    let token_response =
        token::create_token(user_id, vec!["auth".to_string()], &secret)?;

    Response::from_json(&token_response)
}

// ---- Add device (requires existing session) ----

pub async fn add_device_begin(mut req: Request, ctx: RouteContext<()>) -> Result<Response> {
    // Validate existing session token
    let username = match token::validate_from_header(&req, &ctx.env) {
        Ok(sub) => sub,
        Err(e) => {
            let body = ErrorResponse {
                error: format!("authentication required: {e}"),
            };
            return Ok(Response::from_json(&body)?.with_status(401));
        }
    };

    let body: RegisterBeginRequest = req
        .json()
        .await
        .map_err(|e| Error::RustError(format!("invalid request body: {e}")))?;

    // The username in the token must match the request (or we use the token subject directly)
    // For add-device, we use the username from the token's subject as the DO key.
    let _ = body; // body may optionally contain username, but we trust the token
    let stub = user_stub(&ctx, &username)?;

    let do_body = serde_json::json!({ "username": username });
    let internal_req = do_request("/passkey/register/begin", &do_body)?;
    let resp = stub.fetch_with_request(internal_req).await?;

    Ok(resp)
}

pub async fn add_device_finish(mut req: Request, ctx: RouteContext<()>) -> Result<Response> {
    // Validate existing session token
    let username = match token::validate_from_header(&req, &ctx.env) {
        Ok(sub) => sub,
        Err(e) => {
            let body = ErrorResponse {
                error: format!("authentication required: {e}"),
            };
            return Ok(Response::from_json(&body)?.with_status(401));
        }
    };

    let body: RegisterFinishRequest = req
        .json()
        .await
        .map_err(|e| Error::RustError(format!("invalid request body: {e}")))?;

    let stub = user_stub(&ctx, &username)?;

    let do_body = serde_json::json!({ "credential": body.credential });
    let internal_req = do_request("/passkey/register/finish", &do_body)?;
    let resp = stub.fetch_with_request(internal_req).await?;

    Ok(resp)
}

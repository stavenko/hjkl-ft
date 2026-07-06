use worker::*;

use crate::auth_do::create_recovery_hash;
use crate::token;
use crate::types::{ErrorResponse, RecoveryAuthRequest, RecoverySetRequest};
use crate::{auth_do_stub, do_request};

pub async fn set_recovery_key(mut req: Request, ctx: RouteContext<()>) -> Result<Response> {
    // Validate session token
    let user_id = match token::validate_from_header(&req, &ctx.env).await {
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

    // Forward to AuthDO
    let stub = auth_do_stub(&ctx.env)?;
    let do_body = serde_json::json!({
        "user_id": user_id,
        "hash_data": hash_data,
    });
    let internal_req = do_request("/recovery/set", &do_body)?;
    let resp = stub.fetch_with_request(internal_req).await?;

    if resp.status_code() != 200 {
        return Response::error("failed to store recovery hash", 500);
    }

    Response::from_json(&serde_json::json!({ "status": "ok" }))
}

pub async fn authenticate_with_recovery(
    mut req: Request,
    ctx: RouteContext<()>,
) -> Result<Response> {
    let raw_body: serde_json::Value = req
        .json()
        .await
        .map_err(|e| Error::RustError(format!("invalid request body: {e}")))?;

    let body: RecoveryAuthRequest = serde_json::from_value(raw_body.clone())
        .map_err(|e| Error::RustError(format!("invalid request body: {e}")))?;

    let fingerprint = raw_body
        .get("fingerprint")
        .and_then(|v| v.as_str())
        .unwrap_or("");

    // Forward to AuthDO
    let stub = auth_do_stub(&ctx.env)?;
    let do_body = serde_json::json!({
        "user_id": body.user_id,
        "recovery_key": body.recovery_key,
    });
    let internal_req = do_request("/recovery/verify", &do_body)?;
    let mut resp = stub.fetch_with_request(internal_req).await?;

    if resp.status_code() != 200 {
        let body = ErrorResponse {
            error: "invalid recovery key".into(),
        };
        return Ok(Response::from_json(&body)?.with_status(401));
    }

    let result: serde_json::Value = resp.json().await?;
    let valid = result
        .get("valid")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    if !valid {
        let err_body = ErrorResponse {
            error: "invalid recovery key".into(),
        };
        return Ok(Response::from_json(&err_body)?.with_status(401));
    }

    // Issue a new session token
    let secret = token::jwt_secret(&ctx.env).await?;

    let (token_response, token_id) =
        token::create_token(&body.user_id, fingerprint, vec!["auth".to_string()], &secret)?;

    // Store token metadata in DO
    token::store_token_in_do(&ctx.env, &token_id, &body.user_id, fingerprint).await?;

    Response::from_json(&token_response)
}

// ---- Backup phrase (plaintext + reverse index; username-less login) ----

/// JWT-gated: set/replace the caller's backup phrase. Returns the DO status verbatim
/// (`ok` | `taken` | `too_short`) so the client can regenerate on collision.
pub async fn set_phrase(mut req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let user_id = match token::validate_from_header(&req, &ctx.env).await {
        Ok(id) => id,
        Err(e) => {
            let body = ErrorResponse { error: format!("authentication failed: {e}") };
            return Ok(Response::from_json(&body)?.with_status(401));
        }
    };
    let body: serde_json::Value = req
        .json()
        .await
        .map_err(|e| Error::RustError(format!("invalid request body: {e}")))?;
    let phrase = body
        .get("phrase")
        .and_then(|v| v.as_str())
        .ok_or_else(|| Error::RustError("missing phrase".into()))?;

    let stub = auth_do_stub(&ctx.env)?;
    let do_body = serde_json::json!({ "user_id": user_id, "phrase": phrase });
    let internal_req = do_request("/recovery/phrase/set", &do_body)?;
    let mut resp = stub.fetch_with_request(internal_req).await?;
    if resp.status_code() != 200 {
        return Response::error("failed to store backup phrase", 500);
    }
    // Pass the DO status straight through.
    let result: serde_json::Value = resp.json().await?;
    Response::from_json(&result)
}

/// JWT-gated: return the caller's current plaintext phrase for re-display in Settings.
pub async fn get_phrase(req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let user_id = match token::validate_from_header(&req, &ctx.env).await {
        Ok(id) => id,
        Err(e) => {
            let body = ErrorResponse { error: format!("authentication failed: {e}") };
            return Ok(Response::from_json(&body)?.with_status(401));
        }
    };
    let stub = auth_do_stub(&ctx.env)?;
    let do_body = serde_json::json!({ "user_id": user_id });
    let internal_req = do_request("/recovery/phrase/get", &do_body)?;
    let mut resp = stub.fetch_with_request(internal_req).await?;
    if resp.status_code() != 200 {
        return Response::error("failed to read backup phrase", 500);
    }
    let result: serde_json::Value = resp.json().await?;
    Response::from_json(&result)
}

/// PUBLIC (username-less): log in with only the backup phrase. Resolves phrase → user_id
/// in the DO (rate-limited there), then mints a session JWT. Unknown phrase → 401;
/// too many attempts → 429.
pub async fn login_with_phrase(mut req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let raw_body: serde_json::Value = req
        .json()
        .await
        .map_err(|e| Error::RustError(format!("invalid request body: {e}")))?;
    let phrase = raw_body
        .get("phrase")
        .and_then(|v| v.as_str())
        .ok_or_else(|| Error::RustError("missing phrase".into()))?;
    let fingerprint = raw_body.get("fingerprint").and_then(|v| v.as_str()).unwrap_or("");

    let stub = auth_do_stub(&ctx.env)?;
    let do_body = serde_json::json!({ "phrase": phrase });
    let internal_req = do_request("/recovery/phrase/resolve", &do_body)?;
    let mut resp = stub.fetch_with_request(internal_req).await?;

    // Propagate the DO's rate-limit verdict.
    if resp.status_code() == 429 {
        let body = ErrorResponse { error: "too many attempts".into() };
        return Ok(Response::from_json(&body)?.with_status(429));
    }
    if resp.status_code() != 200 {
        return Response::error("phrase login failed", 500);
    }
    let result: serde_json::Value = resp.json().await?;
    let user_id = match result.get("user_id").and_then(|v| v.as_str()) {
        Some(uid) => uid.to_string(),
        None => {
            let body = ErrorResponse { error: "invalid phrase".into() };
            return Ok(Response::from_json(&body)?.with_status(401));
        }
    };

    let secret = token::jwt_secret(&ctx.env).await?;
    let (token_response, token_id) =
        token::create_token(&user_id, fingerprint, vec!["auth".to_string()], &secret)?;
    token::store_token_in_do(&ctx.env, &token_id, &user_id, fingerprint).await?;
    Response::from_json(&token_response)
}

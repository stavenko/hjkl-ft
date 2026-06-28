use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use hmac::{Hmac, Mac};
use sha2::Sha256;
use worker::*;

use crate::token;
use crate::types::{ErrorResponse, PairStatusResponse, PairingStatus};
use crate::{auth_do_stub, do_request};

type HmacSha256 = Hmac<Sha256>;

fn generate_pairing_id() -> String {
    let mut bytes = [0u8; 6];
    getrandom::getrandom(&mut bytes).expect("getrandom failed");
    let encoded = URL_SAFE_NO_PAD.encode(bytes);
    encoded
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .take(8)
        .collect::<String>()
        .to_lowercase()
}

fn generate_secret() -> String {
    let mut bytes = [0u8; 32];
    getrandom::getrandom(&mut bytes).expect("getrandom failed");
    URL_SAFE_NO_PAD.encode(bytes)
}

fn hash_secret(secret: &str) -> String {
    let mut mac =
        HmacSha256::new_from_slice(b"hjkl-pairing-secret").expect("HMAC accepts any key size");
    mac.update(secret.as_bytes());
    let result = mac.finalize().into_bytes();
    URL_SAFE_NO_PAD.encode(result)
}

#[cfg(target_arch = "wasm32")]
fn now_secs() -> i64 {
    (Date::now().as_millis() / 1000) as i64
}

#[cfg(not(target_arch = "wasm32"))]
fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("system clock before UNIX epoch")
        .as_secs() as i64
}

// ---- POST /pair/create (authenticated) ----

pub async fn create_pairing(req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let user_id = match token::validate_from_header(&req, &ctx.env).await {
        Ok(sub) => sub,
        Err(e) => {
            let body = ErrorResponse {
                error: format!("authentication required: {e}"),
            };
            return Ok(Response::from_json(&body)?.with_status(401));
        }
    };

    let pairing_id = generate_pairing_id();
    let secret = generate_secret();
    let secret_hash = hash_secret(&secret);
    let now = now_secs();
    let expires_at = now + 300; // 5 minutes

    let stub = auth_do_stub(&ctx.env)?;
    let do_body = serde_json::json!({
        "pairing_id": pairing_id,
        "secret_hash": secret_hash,
        "user_id": user_id,
        "created_at": now,
        "expires_at": expires_at,
    });
    let internal_req = do_request("/pair/create", &do_body)?;
    let resp = stub.fetch_with_request(internal_req).await?;

    if resp.status_code() != 200 {
        return Response::error("failed to create pairing session", 500);
    }

    Response::from_json(&serde_json::json!({
        "pairing_id": pairing_id,
        "secret": secret,
        "expires_at": expires_at,
    }))
}

// ---- POST /pair/request (no auth, new device) ----

pub async fn request_pairing(_req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let pairing_id = generate_pairing_id();
    let secret = generate_secret();
    let secret_hash = hash_secret(&secret);
    let now = now_secs();
    let expires_at = now + 300;

    let stub = auth_do_stub(&ctx.env)?;
    let do_body = serde_json::json!({
        "pairing_id": pairing_id,
        "secret_hash": secret_hash,
        "created_at": now,
        "expires_at": expires_at,
    });
    let internal_req = do_request("/pair/request", &do_body)?;
    let resp = stub.fetch_with_request(internal_req).await?;

    if resp.status_code() != 200 {
        return Response::error("failed to create pairing request", 500);
    }

    let qr_url = format!("hjkl-pair://{}/{}", pairing_id, secret);

    Response::from_json(&serde_json::json!({
        "pairing_id": pairing_id,
        "secret": secret,
        "qr_url": qr_url,
        "expires_at": expires_at,
    }))
}

// ---- POST /pair/approve (authenticated) ----

pub async fn approve_pairing(mut req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let user_id = match token::validate_from_header(&req, &ctx.env).await {
        Ok(sub) => sub,
        Err(e) => {
            let body = ErrorResponse {
                error: format!("authentication required: {e}"),
            };
            return Ok(Response::from_json(&body)?.with_status(401));
        }
    };

    let body: serde_json::Value = req
        .json()
        .await
        .map_err(|e| Error::RustError(format!("invalid body: {e}")))?;
    let pairing_id = body
        .get("pairing_id")
        .and_then(|v| v.as_str())
        .ok_or_else(|| Error::RustError("missing pairing_id".into()))?;
    let secret = body
        .get("secret")
        .and_then(|v| v.as_str())
        .ok_or_else(|| Error::RustError("missing secret".into()))?;

    let secret_hash = hash_secret(secret);

    let stub = auth_do_stub(&ctx.env)?;
    let do_body = serde_json::json!({
        "pairing_id": pairing_id,
        "secret_hash": secret_hash,
        "user_id": user_id,
    });
    let internal_req = do_request("/pair/approve", &do_body)?;
    let resp = stub.fetch_with_request(internal_req).await?;

    if resp.status_code() != 200 {
        return Ok(resp);
    }

    Response::from_json(&serde_json::json!({ "status": "approved" }))
}

// ---- POST /pair/claim (no auth) ----

/// Unauthenticated status check: new device polls this to see if pairing was approved.
pub async fn check_pairing(mut req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let body: serde_json::Value = req.json().await
        .map_err(|e| Error::RustError(format!("invalid body: {e}")))?;
    let pairing_id = body.get("pairing_id")
        .and_then(|v| v.as_str())
        .ok_or_else(|| Error::RustError("missing pairing_id".into()))?;
    let stub = auth_do_stub(&ctx.env)?;
    let do_body = serde_json::json!({ "pairing_id": pairing_id });
    let internal_req = do_request("/pair/check", &do_body)?;
    let mut resp = stub.fetch_with_request(internal_req).await?;

    if resp.status_code() != 200 {
        let err = ErrorResponse { error: "pairing not found".into() };
        return Ok(Response::from_json(&err)?.with_status(404));
    }

    Ok(resp)
}

pub async fn claim_pairing(mut req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let body: serde_json::Value = req
        .json()
        .await
        .map_err(|e| Error::RustError(format!("invalid request body: {e}")))?;

    let pairing_id = body
        .get("pairing_id")
        .and_then(|v| v.as_str())
        .ok_or_else(|| Error::RustError("missing pairing_id".into()))?;
    let secret = body
        .get("secret")
        .and_then(|v| v.as_str())
        .ok_or_else(|| Error::RustError("missing secret".into()))?;

    let secret_hash = hash_secret(secret);

    let stub = auth_do_stub(&ctx.env)?;
    let do_body = serde_json::json!({
        "pairing_id": pairing_id,
        "secret_hash": secret_hash,
    });
    let internal_req = do_request("/pair/claim", &do_body)?;
    let resp = stub.fetch_with_request(internal_req).await?;

    Ok(resp)
}

// ---- POST /pair/finish (no auth) ----

pub async fn finish_pairing(mut req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let body: serde_json::Value = req
        .json()
        .await
        .map_err(|e| Error::RustError(format!("invalid request body: {e}")))?;

    let pairing_id = body
        .get("pairing_id")
        .and_then(|v| v.as_str())
        .ok_or_else(|| Error::RustError("missing pairing_id".into()))?;
    let credential = body
        .get("credential")
        .cloned()
        .ok_or_else(|| Error::RustError("missing credential".into()))?;
    let fingerprint = body
        .get("fingerprint")
        .and_then(|v| v.as_str())
        .unwrap_or("");

    let stub = auth_do_stub(&ctx.env)?;
    let do_body = serde_json::json!({
        "pairing_id": pairing_id,
        "credential": credential,
    });
    let internal_req = do_request("/pair/finish", &do_body)?;
    let mut resp = stub.fetch_with_request(internal_req).await?;

    if resp.status_code() != 200 {
        return Ok(resp);
    }

    // Parse the DO response to get user_id, then issue JWT
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
        "token": token_response.token,
        "expires_at": token_response.expires_at,
        "user_id": user_id,
    }))
}

// ---- GET /pair/status/:id (authenticated) ----

pub async fn pairing_status(req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let _user_id = match token::validate_from_header(&req, &ctx.env).await {
        Ok(sub) => sub,
        Err(e) => {
            let body = ErrorResponse {
                error: format!("authentication required: {e}"),
            };
            return Ok(Response::from_json(&body)?.with_status(401));
        }
    };

    let pairing_id = ctx
        .param("id")
        .ok_or_else(|| Error::RustError("missing pairing id parameter".into()))?
        .to_string();

    let stub = auth_do_stub(&ctx.env)?;
    let do_body = serde_json::json!({ "pairing_id": pairing_id });
    let internal_req = do_request("/pair/status", &do_body)?;
    let mut resp = stub.fetch_with_request(internal_req).await?;

    if resp.status_code() != 200 {
        let body = ErrorResponse {
            error: "pairing session not found".into(),
        };
        return Ok(Response::from_json(&body)?.with_status(404));
    }

    let do_result: serde_json::Value = resp
        .json()
        .await
        .map_err(|e| Error::RustError(format!("parse DO response: {e}")))?;

    let status: PairingStatus = serde_json::from_value(
        do_result
            .get("status")
            .cloned()
            .ok_or_else(|| Error::RustError("missing status".into()))?,
    )
    .map_err(|e| Error::RustError(format!("parse status: {e}")))?;

    let response = PairStatusResponse { status };
    Response::from_json(&response)
}

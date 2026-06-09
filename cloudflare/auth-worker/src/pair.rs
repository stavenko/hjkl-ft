use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use hmac::{Hmac, Mac};
use sha2::Sha256;
use worker::*;

use crate::token;
use crate::types::{
    ErrorResponse, PairClaimRequest, PairCreateResponse, PairFinishRequest, PairStatusResponse,
    PairingSession, PairingStatus,
};

type HmacSha256 = Hmac<Sha256>;

/// Build an internal POST request to the Durable Object.
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

fn generate_pairing_id() -> String {
    let mut bytes = [0u8; 6];
    getrandom::getrandom(&mut bytes).expect("getrandom failed");
    // Encode 6 bytes to base32-like alphanumeric, take 8 chars
    let encoded = URL_SAFE_NO_PAD.encode(bytes);
    // Filter to alphanumeric only and take 8 chars
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

fn verify_secret(secret: &str, stored_hash: &str) -> bool {
    hash_secret(secret) == stored_hash
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
    // Validate JWT - get user_id from token
    let user_id = match token::validate_from_header(&req, &ctx.env) {
        Ok(sub) => sub,
        Err(e) => {
            let body = ErrorResponse {
                error: format!("authentication required: {e}"),
            };
            return Ok(Response::from_json(&body)?.with_status(401));
        }
    };

    // The JWT sub is the user_id. The DO is keyed by username.
    // Following the same pattern as add_device_begin, we use user_id as the DO key.
    let username = user_id.clone();
    let stub = user_stub(&ctx, &username)?;

    let pairing_id = generate_pairing_id();
    let secret = generate_secret();
    let secret_hash = hash_secret(&secret);
    let now = now_secs();
    let expires_at = now + 300; // 5 minutes

    let do_body = serde_json::json!({
        "pairing_id": pairing_id,
        "secret_hash": secret_hash,
        "username": username,
        "user_id": user_id,
        "created_at": now,
        "expires_at": expires_at,
    });
    let internal_req = do_request("/pairing/create", &do_body)?;
    let resp = stub.fetch_with_request(internal_req).await?;

    if resp.status_code() != 200 {
        return Response::error("failed to create pairing session", 500);
    }

    let response = PairCreateResponse {
        pairing_id,
        secret,
        username,
        expires_at,
    };
    Response::from_json(&response)
}

// ---- POST /pair/request (no auth, new device) ----
// New device generates a pairing request. Stored in a global "pairing-requests" DO.
// Returns { pairing_id, secret, qr_url } — the QR encodes pairing_id + secret.
// Logged-in device scans QR → calls /pair/approve → binds to its user.

pub async fn request_pairing(_req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let pairing_id = generate_pairing_id();
    let secret = generate_secret();
    let secret_hash = hash_secret(&secret);
    let now = now_secs();
    let expires_at = now + 300;

    let stub = {
        let namespace = ctx.env.durable_object("USER_DO")?;
        namespace.id_from_name("__pairing_requests__")?.get_stub()?
    };

    let do_body = serde_json::json!({
        "pairing_id": pairing_id,
        "secret_hash": secret_hash,
        "username": "",
        "user_id": "",
        "created_at": now,
        "expires_at": expires_at,
    });
    let internal_req = do_request("/pairing/create", &do_body)?;
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
// Logged-in device scans QR from new device, approves the pairing by binding it to its user.

pub async fn approve_pairing(mut req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let user_id = match token::validate_from_header(&req, &ctx.env) {
        Ok(sub) => sub,
        Err(e) => {
            let body = ErrorResponse {
                error: format!("authentication required: {e}"),
            };
            return Ok(Response::from_json(&body)?.with_status(401));
        }
    };

    let body: serde_json::Value = req.json().await
        .map_err(|e| Error::RustError(format!("invalid body: {e}")))?;
    let pairing_id = body.get("pairing_id")
        .and_then(|v| v.as_str())
        .ok_or_else(|| Error::RustError("missing pairing_id".into()))?;
    let secret = body.get("secret")
        .and_then(|v| v.as_str())
        .ok_or_else(|| Error::RustError("missing secret".into()))?;

    // Get the pending request from global DO
    let global_stub = {
        let namespace = ctx.env.durable_object("USER_DO")?;
        namespace.id_from_name("__pairing_requests__")?.get_stub()?
    };

    let get_body = serde_json::json!({ "pairing_id": pairing_id });
    let get_req = do_request("/pairing/get", &get_body)?;
    let mut get_resp = global_stub.fetch_with_request(get_req).await?;

    if get_resp.status_code() != 200 {
        let err = ErrorResponse { error: "pairing request not found".into() };
        return Ok(Response::from_json(&err)?.with_status(404));
    }

    let session: PairingSession = get_resp.json().await
        .map_err(|e| Error::RustError(format!("parse session: {e}")))?;

    if session.expires_at < now_secs() {
        let err = ErrorResponse { error: "pairing request expired".into() };
        return Ok(Response::from_json(&err)?.with_status(410));
    }
    if session.status != PairingStatus::Pending {
        let err = ErrorResponse { error: "pairing request already used".into() };
        return Ok(Response::from_json(&err)?.with_status(409));
    }
    if !verify_secret(secret, &session.secret_hash) {
        let err = ErrorResponse { error: "invalid secret".into() };
        return Ok(Response::from_json(&err)?.with_status(403));
    }

    // Mark as claimed in global DO and store the user binding
    let claim_body = serde_json::json!({
        "pairing_id": pairing_id,
        "user_id": user_id,
        "username": user_id,
    });
    let claim_req = do_request("/pairing/approve", &claim_body)?;
    let claim_resp = global_stub.fetch_with_request(claim_req).await?;

    if claim_resp.status_code() != 200 {
        return Response::error("failed to approve pairing", 500);
    }

    Response::from_json(&serde_json::json!({ "status": "approved" }))
}

// ---- POST /pair/claim (no auth) ----

pub async fn claim_pairing(mut req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let body: PairClaimRequest = req
        .json()
        .await
        .map_err(|e| Error::RustError(format!("invalid request body: {e}")))?;

    // Flow A (3-part QR): username is the user DO name
    // Flow B (2-part QR): username is empty → global __pairing_requests__ DO
    let do_name = if body.username.is_empty() { "__pairing_requests__" } else { &body.username };
    let stub = user_stub(&ctx, do_name)?;

    // Get the pairing session to verify the secret
    let get_body = serde_json::json!({ "pairing_id": body.pairing_id });
    let get_req = do_request("/pairing/get", &get_body)?;
    let mut get_resp = stub.fetch_with_request(get_req).await?;

    if get_resp.status_code() != 200 {
        let body = ErrorResponse {
            error: "pairing session not found".into(),
        };
        return Ok(Response::from_json(&body)?.with_status(404));
    }

    let session: PairingSession = get_resp
        .json()
        .await
        .map_err(|e| Error::RustError(format!("parse pairing session: {e}")))?;

    // Check expiry
    let now = now_secs();
    if session.expires_at < now {
        let body = ErrorResponse {
            error: "pairing session expired".into(),
        };
        return Ok(Response::from_json(&body)?.with_status(410));
    }

    // Check status
    if session.status != PairingStatus::Pending {
        let body = ErrorResponse {
            error: "pairing session already claimed or completed".into(),
        };
        return Ok(Response::from_json(&body)?.with_status(409));
    }

    // Verify secret
    if !verify_secret(&body.secret, &session.secret_hash) {
        let body = ErrorResponse {
            error: "invalid pairing secret".into(),
        };
        return Ok(Response::from_json(&body)?.with_status(403));
    }

    // Mark as claimed
    let claim_body = serde_json::json!({ "pairing_id": body.pairing_id });
    let claim_req = do_request("/pairing/claim", &claim_body)?;
    let claim_resp = stub.fetch_with_request(claim_req).await?;

    if claim_resp.status_code() != 200 {
        return Response::error("failed to claim pairing session", 500);
    }

    // Start passkey registration on the USER's DO (not the global pairing DO)
    let user_do_stub = if session.username.is_empty() {
        stub // Flow A: stub is already the user DO
    } else {
        user_stub(&ctx, &session.username)?
    };
    let reg_body = serde_json::json!({ "username": session.username });
    let reg_req = do_request("/passkey/register/begin", &reg_body)?;
    let reg_resp = user_do_stub.fetch_with_request(reg_req).await?;

    Ok(reg_resp)
}

// ---- POST /pair/finish (no auth) ----

pub async fn finish_pairing(mut req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let body: PairFinishRequest = req
        .json()
        .await
        .map_err(|e| Error::RustError(format!("invalid request body: {e}")))?;

    let do_name = if body.username.is_empty() { "__pairing_requests__" } else { &body.username };
    let stub = user_stub(&ctx, do_name)?;

    // Verify pairing session is in claimed state
    let get_body = serde_json::json!({ "pairing_id": body.pairing_id });
    let get_req = do_request("/pairing/get", &get_body)?;
    let mut get_resp = stub.fetch_with_request(get_req).await?;

    if get_resp.status_code() != 200 {
        let err = ErrorResponse {
            error: "pairing session not found".into(),
        };
        return Ok(Response::from_json(&err)?.with_status(404));
    }

    let session: PairingSession = get_resp
        .json()
        .await
        .map_err(|e| Error::RustError(format!("parse pairing session: {e}")))?;

    if session.status != PairingStatus::Claimed {
        let err = ErrorResponse {
            error: "pairing session not in claimed state".into(),
        };
        return Ok(Response::from_json(&err)?.with_status(409));
    }

    // Complete passkey registration on the USER's DO
    let user_do_stub = if session.username.is_empty() {
        user_stub(&ctx, do_name)?
    } else {
        user_stub(&ctx, &session.username)?
    };
    let finish_body = serde_json::json!({ "credential": body.credential });
    let finish_req = do_request("/passkey/register/finish", &finish_body)?;
    let resp = user_do_stub.fetch_with_request(finish_req).await?;

    if resp.status_code() != 200 {
        return Ok(resp);
    }

    // Mark pairing as completed
    let complete_body = serde_json::json!({ "pairing_id": body.pairing_id });
    let complete_req = do_request("/pairing/complete", &complete_body)?;
    let complete_resp = stub.fetch_with_request(complete_req).await?;

    if complete_resp.status_code() != 200 {
        return Response::error("failed to mark pairing as completed", 500);
    }

    // Issue JWT for the user
    let secret = ctx
        .env
        .secret("JWT_SECRET")
        .map(|s| s.to_string())
        .map_err(|_| Error::RustError("JWT_SECRET not configured".into()))?;

    let token_response =
        token::create_token(&session.user_id, vec!["auth".to_string()], &secret)?;

    let result = serde_json::json!({
        "token": token_response.token,
        "expires_at": token_response.expires_at,
        "user_id": session.user_id,
    });
    Response::from_json(&result)
}

// ---- GET /pair/status/:id (authenticated) ----

pub async fn pairing_status(req: Request, ctx: RouteContext<()>) -> Result<Response> {
    // Validate JWT
    let username = match token::validate_from_header(&req, &ctx.env) {
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

    let stub = user_stub(&ctx, &username)?;

    let get_body = serde_json::json!({ "pairing_id": pairing_id });
    let get_req = do_request("/pairing/get", &get_body)?;
    let mut get_resp = stub.fetch_with_request(get_req).await?;

    if get_resp.status_code() != 200 {
        let body = ErrorResponse {
            error: "pairing session not found".into(),
        };
        return Ok(Response::from_json(&body)?.with_status(404));
    }

    let session: PairingSession = get_resp
        .json()
        .await
        .map_err(|e| Error::RustError(format!("parse pairing session: {e}")))?;

    let response = PairStatusResponse {
        status: session.status,
    };
    Response::from_json(&response)
}

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use hmac::{Hmac, Mac};
use sha2::Sha256;
use worker::*;

use crate::types::{ErrorResponse, TokenClaims, TokenResponse};
use crate::{auth_do_stub, do_request};

type HmacSha256 = Hmac<Sha256>;

const JWT_HEADER: &str = r#"{"alg":"HS256","typ":"JWT"}"#;

/// Far-future expiry: year 2100 (epoch seconds).
const FAR_FUTURE_EXP: i64 = 4_102_444_800;

#[cfg(target_arch = "wasm32")]
fn current_timestamp() -> i64 {
    (Date::now().as_millis() / 1000) as i64
}

#[cfg(not(target_arch = "wasm32"))]
fn current_timestamp() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("system clock before UNIX epoch")
        .as_secs() as i64
}

fn now_secs() -> i64 {
    current_timestamp()
}

fn base64url_encode(data: &[u8]) -> String {
    URL_SAFE_NO_PAD.encode(data)
}

fn base64url_decode(s: &str) -> std::result::Result<Vec<u8>, base64::DecodeError> {
    URL_SAFE_NO_PAD.decode(s)
}

fn sign_jwt(claims: &TokenClaims, secret: &str) -> Result<String> {
    let header_b64 = base64url_encode(JWT_HEADER.as_bytes());
    let claims_json =
        serde_json::to_vec(claims).map_err(|e| Error::RustError(format!("serialize claims: {e}")))?;
    let claims_b64 = base64url_encode(&claims_json);

    let signing_input = format!("{header_b64}.{claims_b64}");

    let mut mac = HmacSha256::new_from_slice(secret.as_bytes())
        .map_err(|e| Error::RustError(format!("hmac init: {e}")))?;
    mac.update(signing_input.as_bytes());
    let signature = mac.finalize().into_bytes();
    let sig_b64 = base64url_encode(&signature);

    Ok(format!("{signing_input}.{sig_b64}"))
}

fn verify_and_decode(token: &str, secret: &str) -> Result<TokenClaims> {
    let parts: Vec<&str> = token.split('.').collect();
    if parts.len() != 3 {
        return Err(Error::RustError("invalid token format".into()));
    }

    let signing_input = format!("{}.{}", parts[0], parts[1]);
    let provided_sig =
        base64url_decode(parts[2]).map_err(|e| Error::RustError(format!("decode sig: {e}")))?;

    let mut mac = HmacSha256::new_from_slice(secret.as_bytes())
        .map_err(|e| Error::RustError(format!("hmac init: {e}")))?;
    mac.update(signing_input.as_bytes());
    mac.verify_slice(&provided_sig)
        .map_err(|_| Error::RustError("invalid token signature".into()))?;

    let claims_bytes = base64url_decode(parts[1])
        .map_err(|e| Error::RustError(format!("decode claims: {e}")))?;
    let claims: TokenClaims = serde_json::from_slice(&claims_bytes)
        .map_err(|e| Error::RustError(format!("parse claims: {e}")))?;

    // Tokens use far-future exp; this check is kept as a safeguard
    let now = now_secs();
    if claims.exp < now {
        return Err(Error::RustError("token expired".into()));
    }

    Ok(claims)
}

/// Create a signed JWT for the given user. The token never expires (far-future exp).
/// A unique `token_id` is embedded in the claims so the token can be identified and revoked
/// via DO storage.
///
/// Returns the signed JWT string plus the generated token_id.
pub fn create_token(
    user_id: &str,
    _fingerprint: &str,
    capabilities: Vec<String>,
    secret: &str,
) -> Result<(TokenResponse, String)> {
    let now = now_secs();
    let token_id = uuid::Uuid::new_v4().to_string();

    let claims = TokenClaims {
        sub: user_id.to_string(),
        iat: now,
        exp: FAR_FUTURE_EXP,
        caps: capabilities,
        token_id: token_id.clone(),
    };

    let token = sign_jwt(&claims, secret)?;
    let resp = TokenResponse {
        token,
        expires_at: FAR_FUTURE_EXP,
    };
    Ok((resp, token_id))
}

/// Store token metadata in the AuthDO.
/// Must be called after `create_token` to persist the token for later validation.
pub async fn store_token_in_do(
    env: &Env,
    token_id: &str,
    user_id: &str,
    fingerprint: &str,
) -> Result<()> {
    let stub = auth_do_stub(env)?;
    let now = now_secs();
    let do_body = serde_json::json!({
        "token_id": token_id,
        "user_id": user_id,
        "fingerprint": fingerprint,
        "created_at": now,
    });
    let internal_req = do_request("/token/store", &do_body)?;
    let resp = stub.fetch_with_request(internal_req).await?;
    if resp.status_code() != 200 {
        return Err(Error::RustError("failed to store token in DO".into()));
    }
    Ok(())
}

pub fn create_ephemeral_token(
    user_id: &str,
    capability: &str,
    secret: &str,
) -> Result<TokenResponse> {
    let now = now_secs();
    let exp = now + 300; // 5 minutes

    let claims = TokenClaims {
        sub: user_id.to_string(),
        iat: now,
        exp,
        caps: vec![capability.to_string()],
        token_id: uuid::Uuid::new_v4().to_string(),
    };

    let jwt = sign_jwt(&claims, secret)?;
    Ok(TokenResponse {
        token: jwt,
        expires_at: exp,
    })
}

fn extract_bearer_token(req: &Request) -> Result<String> {
    let headers = req.headers();
    let auth_header = headers
        .get("Authorization")
        .map_err(|e| Error::RustError(format!("read header: {e}")))?
        .ok_or_else(|| Error::RustError("missing Authorization header".into()))?;

    let token = auth_header
        .strip_prefix("Bearer ")
        .ok_or_else(|| Error::RustError("Authorization header must be Bearer <token>".into()))?;

    Ok(token.to_string())
}

pub fn validate_token_from_request(req: &Request, secret: &str) -> Result<TokenClaims> {
    let token = extract_bearer_token(req)?;
    verify_and_decode(&token, secret)
}

/// Validate a session token from the Authorization header using JWT_SECRET from env.
/// This only verifies the JWT signature -- it does NOT check DO storage.
/// For full validation (including revocation check), use `validate_from_header_full`.
pub fn validate_from_header(req: &Request, env: &Env) -> Result<String> {
    let secret = env
        .secret("JWT_SECRET")
        .map(|s| s.to_string())
        .map_err(|_| Error::RustError("JWT_SECRET not configured".into()))?;
    let claims = validate_token_from_request(req, &secret)?;
    Ok(claims.sub)
}

/// Full async validation: verify JWT signature AND check that the token still
/// exists in DO storage (not revoked). Updates `last_used_at`.
pub async fn validate_from_header_full(req: &Request, env: &Env) -> Result<String> {
    let secret = env
        .secret("JWT_SECRET")
        .map(|s| s.to_string())
        .map_err(|_| Error::RustError("JWT_SECRET not configured".into()))?;
    let claims = validate_token_from_request(req, &secret)?;

    // Check token exists in DO storage
    let stub = auth_do_stub(env)?;
    let do_body = serde_json::json!({ "token_id": claims.token_id });
    let internal_req = do_request("/token/validate", &do_body)?;
    let resp = stub.fetch_with_request(internal_req).await?;
    if resp.status_code() != 200 {
        return Err(Error::RustError("token has been revoked".into()));
    }

    Ok(claims.sub)
}

/// POST /token/validate -- checks JWT signature and DO storage existence.
pub async fn validate_token(req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let secret = ctx
        .env
        .secret("JWT_SECRET")
        .map(|s| s.to_string())
        .map_err(|_| Error::RustError("JWT_SECRET not configured".into()))?;

    let claims = match validate_token_from_request(&req, &secret) {
        Ok(c) => c,
        Err(e) => {
            let body = ErrorResponse {
                error: e.to_string(),
            };
            return Ok(Response::from_json(&body)?.with_status(401));
        }
    };

    // Verify token exists in DO storage (not revoked) and update last_used_at
    let stub = auth_do_stub(&ctx.env)?;
    let do_body = serde_json::json!({ "token_id": claims.token_id });
    let internal_req = do_request("/token/validate", &do_body)?;
    let resp = stub.fetch_with_request(internal_req).await?;
    if resp.status_code() != 200 {
        let body = ErrorResponse {
            error: "token has been revoked".into(),
        };
        return Ok(Response::from_json(&body)?.with_status(401));
    }

    Response::from_json(&claims)
}

/// GET /tokens -- returns all active tokens for the authenticated user.
pub async fn list_tokens(req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let user_id = match validate_from_header(&req, &ctx.env) {
        Ok(sub) => sub,
        Err(e) => {
            let body = ErrorResponse {
                error: format!("authentication required: {e}"),
            };
            return Ok(Response::from_json(&body)?.with_status(401));
        }
    };

    let stub = auth_do_stub(&ctx.env)?;
    let do_body = serde_json::json!({ "user_id": user_id });
    let internal_req = do_request("/token/list", &do_body)?;
    let resp = stub.fetch_with_request(internal_req).await?;

    Ok(resp)
}

#[cfg(test)]
mod tests {
    use super::*;

    const TEST_SECRET: &str = "test-secret-key-for-unit-tests";

    #[test]
    fn test_create_and_validate_token() {
        let user_id = "user-123";
        let caps = vec!["auth".to_string(), "read".to_string()];

        let (token_response, token_id) =
            create_token(user_id, "test-fingerprint", caps.clone(), TEST_SECRET)
                .expect("create_token should succeed");

        assert!(!token_response.token.is_empty());
        assert!(!token_id.is_empty());
        // Token should not expire (far-future)
        assert_eq!(token_response.expires_at, FAR_FUTURE_EXP);

        let claims = verify_and_decode(&token_response.token, TEST_SECRET)
            .expect("verify_and_decode should succeed");

        assert_eq!(claims.sub, user_id);
        assert_eq!(claims.caps, caps);
        assert_eq!(claims.token_id, token_id);
        assert!(claims.iat <= current_timestamp());
        assert_eq!(claims.exp, FAR_FUTURE_EXP);
    }

    #[test]
    fn test_expired_token() {
        let now = current_timestamp();
        let claims = TokenClaims {
            sub: "user-456".to_string(),
            iat: now - 7200,
            exp: now - 3600, // expired 1 hour ago
            caps: vec!["auth".to_string()],
            token_id: "tok-test".to_string(),
        };

        let token = sign_jwt(&claims, TEST_SECRET).expect("sign_jwt should succeed");
        let result = verify_and_decode(&token, TEST_SECRET);

        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("expired"),
            "error should mention expiry, got: {err}"
        );
    }

    #[test]
    fn test_invalid_signature() {
        let (token_response, _) =
            create_token("user-789", "fp", vec!["auth".to_string()], TEST_SECRET)
                .expect("create_token should succeed");

        let result = verify_and_decode(&token_response.token, "wrong-secret");

        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("signature"),
            "error should mention signature, got: {err}"
        );
    }

    #[test]
    fn test_ephemeral_token() {
        let user_id = "user-ephemeral";
        let capability = "upload";

        let token_response = create_ephemeral_token(user_id, capability, TEST_SECRET)
            .expect("create_ephemeral_token should succeed");

        let now = current_timestamp();
        // Ephemeral tokens expire in 5 minutes (300 seconds)
        assert!(token_response.expires_at <= now + 300);
        assert!(token_response.expires_at > now);

        let claims = verify_and_decode(&token_response.token, TEST_SECRET)
            .expect("verify_and_decode should succeed");

        assert_eq!(claims.sub, user_id);
        assert_eq!(claims.caps.len(), 1);
        assert_eq!(claims.caps[0], capability);
    }
}

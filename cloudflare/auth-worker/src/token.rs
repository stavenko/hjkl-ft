use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use hmac::{Hmac, Mac};
use sha2::Sha256;
use worker::*;

use crate::types::{ErrorResponse, TokenClaims, TokenResponse};

type HmacSha256 = Hmac<Sha256>;

const JWT_HEADER: &str = r#"{"alg":"HS256","typ":"JWT"}"#;

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

    let now = now_secs();
    if claims.exp < now {
        return Err(Error::RustError("token expired".into()));
    }

    Ok(claims)
}

pub fn create_token(
    user_id: &str,
    capabilities: Vec<String>,
    secret: &str,
) -> Result<TokenResponse> {
    let now = now_secs();
    let exp = now + 3600; // 1 hour

    let claims = TokenClaims {
        sub: user_id.to_string(),
        iat: now,
        exp,
        caps: capabilities,
    };

    let token = sign_jwt(&claims, secret)?;
    Ok(TokenResponse {
        token,
        expires_at: exp,
    })
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
    };

    let token = sign_jwt(&claims, secret)?;
    Ok(TokenResponse {
        token,
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
/// Returns the user_id (subject claim) on success.
pub fn validate_from_header(req: &Request, env: &Env) -> Result<String> {
    let secret = env
        .secret("JWT_SECRET")
        .map(|s| s.to_string())
        .map_err(|_| Error::RustError("JWT_SECRET not configured".into()))?;
    let claims = validate_token_from_request(req, &secret)?;
    Ok(claims.sub)
}

pub async fn validate_token(req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let secret = ctx
        .env
        .secret("JWT_SECRET")
        .map(|s| s.to_string())
        .map_err(|_| Error::RustError("JWT_SECRET not configured".into()))?;

    match validate_token_from_request(&req, &secret) {
        Ok(claims) => Response::from_json(&claims),
        Err(e) => {
            let body = ErrorResponse {
                error: e.to_string(),
            };
            let resp = Response::from_json(&body)?;
            Ok(resp.with_status(401))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TEST_SECRET: &str = "test-secret-key-for-unit-tests";

    #[test]
    fn test_create_and_validate_token() {
        let user_id = "user-123";
        let caps = vec!["auth".to_string(), "read".to_string()];

        let token_response = create_token(user_id, caps.clone(), TEST_SECRET)
            .expect("create_token should succeed");

        assert!(!token_response.token.is_empty());
        assert!(token_response.expires_at > current_timestamp());

        let claims = verify_and_decode(&token_response.token, TEST_SECRET)
            .expect("verify_and_decode should succeed");

        assert_eq!(claims.sub, user_id);
        assert_eq!(claims.caps, caps);
        assert!(claims.iat <= current_timestamp());
        assert!(claims.exp > current_timestamp());
    }

    #[test]
    fn test_expired_token() {
        let now = current_timestamp();
        let claims = TokenClaims {
            sub: "user-456".to_string(),
            iat: now - 7200,
            exp: now - 3600, // expired 1 hour ago
            caps: vec!["auth".to_string()],
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
        let token_response = create_token("user-789", vec!["auth".to_string()], TEST_SECRET)
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

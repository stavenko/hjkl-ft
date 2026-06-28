use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use hmac::{Hmac, Mac};
use sha2::Sha256;
use worker::*;

use crate::types::TokenClaims;

type HmacSha256 = Hmac<Sha256>;

/// Resolve a secret, preferring the Cloudflare Secrets Store binding (prod) and
/// falling back to a per-worker secret / [vars] value (dev/test). In dev there is
/// NO Store binding → `env.secret_store` errs → we fall back to the [vars] value,
/// so nothing dev-side changes. In prod the Store binding returns the global value.
/// Returns Err with a clear MISCONFIGURED message when the Store binding is
/// present but unresolvable, or when the secret is configured nowhere.
pub async fn secret_or_var(env: &Env, name: &str) -> std::result::Result<String, String> {
    match env.secret_store(name) {
        Ok(store) => match store.get().await {
            Ok(Some(v)) if !v.is_empty() => Ok(v),
            Ok(_) => Err(format!(
                "MISCONFIGURED: Secrets Store binding '{name}' is empty/unset"
            )),
            Err(e) => Err(format!(
                "MISCONFIGURED: Secrets Store binding '{name}' get() failed: {e:?}"
            )),
        },
        Err(_) => env
            .secret(name)
            .map(|s| s.to_string())
            .ok()
            .or_else(|| env.var(name).map(|v| v.to_string()).ok())
            .ok_or_else(|| {
                format!("MISCONFIGURED: '{name}' not set (no Secrets Store binding and no var/secret)")
            }),
    }
}

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

fn base64url_decode(s: &str) -> std::result::Result<Vec<u8>, base64::DecodeError> {
    URL_SAFE_NO_PAD.decode(s)
}

/// Verify the HMAC-SHA256 signature of a JWT and decode its claims.
/// Ported verbatim from auth-worker/src/token.rs (verify path only).
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

    // Tokens use far-future exp; this check is kept as a safeguard.
    let now = now_secs();
    if claims.exp < now {
        return Err(Error::RustError("token expired".into()));
    }

    Ok(claims)
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
/// Signature-only — support-worker has no AuthDO, so there is no revocation check.
/// Returns the authenticated subject (`claims.sub`).
pub async fn validate_from_header(req: &Request, env: &Env) -> Result<String> {
    let secret = secret_or_var(env, "JWT_SECRET")
        .await
        .map_err(Error::RustError)?;
    let claims = validate_token_from_request(req, &secret)?;
    Ok(claims.sub)
}

#[cfg(test)]
mod tests {
    use super::*;

    const TEST_SECRET: &str = "test-secret-key-for-unit-tests";

    fn base64url_encode(data: &[u8]) -> String {
        URL_SAFE_NO_PAD.encode(data)
    }

    fn sign(claims: &TokenClaims, secret: &str) -> String {
        let header_b64 = base64url_encode(br#"{"alg":"HS256","typ":"JWT"}"#);
        let claims_b64 = base64url_encode(&serde_json::to_vec(claims).unwrap());
        let signing_input = format!("{header_b64}.{claims_b64}");
        let mut mac = HmacSha256::new_from_slice(secret.as_bytes()).unwrap();
        mac.update(signing_input.as_bytes());
        let sig = mac.finalize().into_bytes();
        format!("{signing_input}.{}", base64url_encode(&sig))
    }

    fn claims(sub: &str, exp: i64) -> TokenClaims {
        TokenClaims {
            sub: sub.to_string(),
            iat: 0,
            exp,
            caps: vec![],
            token_id: Some("t-1".into()),
        }
    }

    #[test]
    fn verifies_valid_token() {
        let token = sign(&claims("user-A", 4_102_444_800), TEST_SECRET);
        let decoded = verify_and_decode(&token, TEST_SECRET).expect("should verify");
        assert_eq!(decoded.sub, "user-A");
    }

    #[test]
    fn rejects_wrong_secret() {
        let token = sign(&claims("user-A", 4_102_444_800), TEST_SECRET);
        assert!(verify_and_decode(&token, "wrong").is_err());
    }

    #[test]
    fn rejects_expired() {
        let token = sign(&claims("user-A", 1), TEST_SECRET);
        let err = verify_and_decode(&token, TEST_SECRET).unwrap_err().to_string();
        assert!(err.contains("expired"));
    }

    #[test]
    fn rejects_malformed() {
        assert!(verify_and_decode("not.a.jwt.x", TEST_SECRET).is_err());
        assert!(verify_and_decode("notajwt", TEST_SECRET).is_err());
    }
}

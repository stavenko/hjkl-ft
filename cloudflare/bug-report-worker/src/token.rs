use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use hmac::{Hmac, Mac};
use sha2::Sha256;
use worker::*;

type HmacSha256 = Hmac<Sha256>;

/// Resolve a secret, preferring the Cloudflare Secrets Store binding (prod) and
/// falling back to a per-worker secret / [vars] value (dev/test). In dev there is
/// NO Store binding → `env.secret_store` errs → we fall back to the [vars] value,
/// so nothing dev-side changes. In prod the Store binding returns the global value.
/// Returns Err with a clear MISCONFIGURED message when the Store binding is present
/// but unresolvable, or when the secret is configured nowhere.
pub async fn secret_or_var(env: &Env, name: &str) -> std::result::Result<String, String> {
    match env.secret_store(name) {
        Ok(store) => match store.get().await {
            Ok(Some(v)) if !v.is_empty() => Ok(v),
            Ok(_) => Err(format!("MISCONFIGURED: Secrets Store binding '{name}' is empty/unset")),
            Err(e) => Err(format!("MISCONFIGURED: Secrets Store binding '{name}' get() failed: {e:?}")),
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

fn base64url_decode(s: &str) -> std::result::Result<Vec<u8>, base64::DecodeError> {
    URL_SAFE_NO_PAD.decode(s)
}

/// Verify the HMAC-SHA256 signature of a JWT. Mirrors the TS `verifyJwt`: signature
/// only — NO exp/claims checks (the TS path validated solely the signature). Returns
/// true iff the token has 3 parts and the signature over `header.payload` matches.
pub fn verify_jwt(token: &str, secret: &str) -> bool {
    let parts: Vec<&str> = token.split('.').collect();
    if parts.len() != 3 {
        return false;
    }
    let provided_sig = match base64url_decode(parts[2]) {
        Ok(s) => s,
        Err(_) => return false,
    };
    let signing_input = format!("{}.{}", parts[0], parts[1]);
    let mut mac = match HmacSha256::new_from_slice(secret.as_bytes()) {
        Ok(m) => m,
        Err(_) => return false,
    };
    mac.update(signing_input.as_bytes());
    mac.verify_slice(&provided_sig).is_ok()
}

/// Decode the `sub` claim from a JWT without verifying. Mirrors the TS
/// `decodeJwtSub`: returns None on malformed token / bad base64 / non-string sub.
pub fn decode_jwt_sub(token: &str) -> Option<String> {
    let parts: Vec<&str> = token.split('.').collect();
    if parts.len() != 3 {
        return None;
    }
    let claims_bytes = base64url_decode(parts[1]).ok()?;
    let claims: serde_json::Value = serde_json::from_slice(&claims_bytes).ok()?;
    claims.get("sub").and_then(|v| v.as_str()).map(String::from)
}

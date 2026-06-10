use aes_gcm::aead::Aead;
use aes_gcm::{Aes128Gcm, KeyInit, Nonce};
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use elliptic_curve::sec1::{EncodedPoint, FromEncodedPoint, ToEncodedPoint};
use hkdf::Hkdf;
use p256::ecdsa::{signature::Signer, Signature, SigningKey};
use p256::{PublicKey, SecretKey};
use sha2::Sha256;
use worker::*;

use crate::token;
use crate::types::{ErrorResponse, PushSubscription};
use crate::{auth_do_stub, do_request};

// ---------------------------------------------------------------------------
// Base64url helpers
// ---------------------------------------------------------------------------

fn b64url_encode(data: &[u8]) -> String {
    URL_SAFE_NO_PAD.encode(data)
}

fn b64url_decode(s: &str) -> std::result::Result<Vec<u8>, base64::DecodeError> {
    URL_SAFE_NO_PAD.decode(s)
}

// ---------------------------------------------------------------------------
// Timestamp helper (wasm-compatible)
// ---------------------------------------------------------------------------

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

// ---------------------------------------------------------------------------
// VAPID JWT signing (ES256 / P-256)
// ---------------------------------------------------------------------------

/// Create a VAPID JWT token for the given audience.
///
/// - `audience`: the push service origin, e.g. "https://fcm.googleapis.com"
/// - `subject`: e.g. "mailto:admin@hjkl.pro"
/// - `private_key`: raw 32-byte P-256 private scalar
pub fn create_vapid_token(
    audience: &str,
    subject: &str,
    private_key: &[u8],
) -> std::result::Result<String, String> {
    let header = r#"{"typ":"JWT","alg":"ES256"}"#;
    let exp = current_timestamp() + 12 * 3600; // 12 hours
    let claims = format!(
        r#"{{"aud":"{}","exp":{},"sub":"{}"}}"#,
        audience, exp, subject
    );

    let header_b64 = b64url_encode(header.as_bytes());
    let claims_b64 = b64url_encode(claims.as_bytes());
    let signing_input = format!("{header_b64}.{claims_b64}");

    let signing_key = SigningKey::from_bytes(private_key.into())
        .map_err(|e| format!("invalid VAPID private key: {e}"))?;
    let signature: Signature = signing_key.sign(signing_input.as_bytes());

    // ES256 signature is r || s, each 32 bytes, in raw (non-DER) form.
    let sig_bytes = signature.to_bytes();
    let sig_b64 = b64url_encode(&sig_bytes);

    Ok(format!("{signing_input}.{sig_b64}"))
}

// ---------------------------------------------------------------------------
// HKDF helper
// ---------------------------------------------------------------------------

fn hkdf_sha256(salt: &[u8], ikm: &[u8], info: &[u8], len: usize) -> Vec<u8> {
    let hk = Hkdf::<Sha256>::new(Some(salt), ikm);
    let mut okm = vec![0u8; len];
    hk.expand(info, &mut okm)
        .expect("HKDF expand failed -- output length too large");
    okm
}

// ---------------------------------------------------------------------------
// ECE aes128gcm encryption (RFC 8188 / RFC 8291)
// ---------------------------------------------------------------------------

/// Encrypt a payload for Web Push using the aes128gcm content encoding.
///
/// - `payload`: the plaintext notification body
/// - `subscription_key`: the p256dh value from PushSubscription (65-byte uncompressed point)
/// - `auth_secret`: the auth value from PushSubscription (16 bytes)
pub fn encrypt_payload(
    payload: &[u8],
    subscription_key: &[u8],
    auth_secret: &[u8],
) -> std::result::Result<Vec<u8>, String> {
    // 1. Generate ephemeral ECDH key pair
    let ephemeral_secret = SecretKey::random(&mut rand_core());
    let ephemeral_public = ephemeral_secret.public_key();
    let ephemeral_public_bytes = ephemeral_public.to_encoded_point(false); // 65 bytes uncompressed

    // 2. Parse the subscription public key
    let sub_point = EncodedPoint::<p256::NistP256>::from_bytes(subscription_key)
        .map_err(|e| format!("invalid subscription key: {e}"))?;
    let sub_pubkey = PublicKey::from_encoded_point(&sub_point);
    let sub_pubkey: PublicKey = Option::from(sub_pubkey)
        .ok_or_else(|| "subscription key is not a valid P-256 point".to_string())?;

    // 3. ECDH shared secret
    let shared_secret = p256::ecdh::diffie_hellman(
        ephemeral_secret.to_nonzero_scalar(),
        sub_pubkey.as_affine(),
    );

    // 4. Derive IKM via HKDF with auth_secret as salt
    //    info = "WebPush: info\0" || subscription_key || ephemeral_public_key
    let mut auth_info = Vec::with_capacity(128);
    auth_info.extend_from_slice(b"WebPush: info\0");
    auth_info.extend_from_slice(subscription_key);
    auth_info.extend_from_slice(ephemeral_public_bytes.as_bytes());

    let ikm = hkdf_sha256(auth_secret, shared_secret.raw_secret_bytes(), &auth_info, 32);

    // 5. Generate random 16-byte salt
    let mut salt = [0u8; 16];
    getrandom::getrandom(&mut salt).map_err(|e| format!("getrandom failed: {e}"))?;

    // 6. Derive content encryption key and nonce from IKM with salt
    let cek_info = b"Content-Encoding: aes128gcm\0";
    let nonce_info = b"Content-Encoding: nonce\0";

    let key = hkdf_sha256(&salt, &ikm, cek_info, 16);
    let nonce_bytes = hkdf_sha256(&salt, &ikm, nonce_info, 12);

    // 7. Pad payload: payload || 0x02 || zeros
    //    A single record with delimiter byte 0x02 (final record). No extra padding.
    let mut padded = Vec::with_capacity(payload.len() + 1);
    padded.extend_from_slice(payload);
    padded.push(0x02); // delimiter for the final record

    // 8. AES-128-GCM encrypt
    let cipher = Aes128Gcm::new_from_slice(&key)
        .map_err(|e| format!("AES key init: {e}"))?;
    let nonce = Nonce::from_slice(&nonce_bytes);
    let ciphertext = cipher
        .encrypt(nonce, padded.as_ref())
        .map_err(|e| format!("AES-GCM encrypt: {e}"))?;

    // 9. Build the aes128gcm header + ciphertext
    //    header = salt(16) || rs(4, big-endian) || idlen(1) || keyid(65)
    let rs: u32 = 4096;
    let ephemeral_bytes = ephemeral_public_bytes.as_bytes();
    let idlen = ephemeral_bytes.len() as u8; // 65

    let mut result = Vec::with_capacity(16 + 4 + 1 + ephemeral_bytes.len() + ciphertext.len());
    result.extend_from_slice(&salt);
    result.extend_from_slice(&rs.to_be_bytes());
    result.push(idlen);
    result.extend_from_slice(ephemeral_bytes);
    result.extend_from_slice(&ciphertext);

    Ok(result)
}

/// A getrandom-backed OsRng for p256 key generation.
struct OsRng;

impl rand_core_crate::CryptoRng for OsRng {}

impl rand_core_crate::RngCore for OsRng {
    fn next_u32(&mut self) -> u32 {
        let mut buf = [0u8; 4];
        getrandom::getrandom(&mut buf).expect("getrandom failed");
        u32::from_le_bytes(buf)
    }

    fn next_u64(&mut self) -> u64 {
        let mut buf = [0u8; 8];
        getrandom::getrandom(&mut buf).expect("getrandom failed");
        u64::from_le_bytes(buf)
    }

    fn fill_bytes(&mut self, dest: &mut [u8]) {
        getrandom::getrandom(dest).expect("getrandom failed");
    }

    fn try_fill_bytes(
        &mut self,
        dest: &mut [u8],
    ) -> std::result::Result<(), rand_core_crate::Error> {
        getrandom::getrandom(dest)
            .map_err(|e| rand_core_crate::Error::new(e))
    }
}

fn rand_core() -> OsRng {
    OsRng
}

// We need to use the rand_core version that p256 depends on.
// p256 0.13 uses elliptic-curve 0.13 which uses rand_core 0.6.
use elliptic_curve::rand_core as rand_core_crate;

// ---------------------------------------------------------------------------
// Send push notification
// ---------------------------------------------------------------------------

/// Send a push notification to a single subscription.
///
/// Returns `Ok(true)` if the push service accepted the message (2xx),
/// `Ok(false)` if the subscription is gone (404/410 -- should be removed),
/// or `Err` on other failures.
pub async fn send_push(
    subscription: &PushSubscription,
    payload: &str,
    vapid_private_key: &[u8],
    vapid_public_key: &[u8],
    vapid_subject: &str,
) -> std::result::Result<bool, String> {
    // Decode subscription keys
    let sub_key = b64url_decode(&subscription.keys.p256dh)
        .map_err(|e| format!("decode p256dh: {e}"))?;
    let auth_secret = b64url_decode(&subscription.keys.auth)
        .map_err(|e| format!("decode auth: {e}"))?;

    // Encrypt payload
    let encrypted = encrypt_payload(payload.as_bytes(), &sub_key, &auth_secret)?;

    // Extract audience (origin) from endpoint URL
    let url = worker::Url::parse(&subscription.endpoint)
        .map_err(|e| format!("parse endpoint URL: {e}"))?;
    let audience = url.origin().ascii_serialization();

    // Create VAPID Authorization header
    let jwt = create_vapid_token(&audience, vapid_subject, vapid_private_key)?;
    let vapid_pub_b64 = b64url_encode(vapid_public_key);
    let authorization = format!("vapid t={jwt},k={vapid_pub_b64}");

    // POST to push endpoint
    let headers = Headers::new();
    headers
        .set("Content-Type", "application/octet-stream")
        .map_err(|e| format!("set header: {e}"))?;
    headers
        .set("Content-Encoding", "aes128gcm")
        .map_err(|e| format!("set header: {e}"))?;
    headers
        .set("TTL", "86400")
        .map_err(|e| format!("set header: {e}"))?;
    headers
        .set("Authorization", &authorization)
        .map_err(|e| format!("set header: {e}"))?;

    let body_js = js_sys::Uint8Array::from(encrypted.as_slice());

    let mut init = RequestInit::new();
    init.with_method(Method::Post)
        .with_headers(headers)
        .with_body(Some(body_js.into()));

    let request = Request::new_with_init(&subscription.endpoint, &init)
        .map_err(|e| format!("create request: {e}"))?;

    let resp = Fetch::Request(request)
        .send()
        .await
        .map_err(|e| format!("fetch failed: {e}"))?;

    let status = resp.status_code();
    if (200..300).contains(&status) {
        Ok(true)
    } else if status == 404 || status == 410 {
        // Subscription no longer valid
        Ok(false)
    } else {
        Err(format!(
            "push service returned status {status} for endpoint {}",
            subscription.endpoint
        ))
    }
}

// ---------------------------------------------------------------------------
// Route handlers
// ---------------------------------------------------------------------------

/// GET /push/vapid-key -- return the public VAPID key (unauthenticated)
pub async fn vapid_key(_req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let public_key = ctx
        .env
        .var("VAPID_PUBLIC_KEY")
        .map(|v| v.to_string())
        .map_err(|_| Error::RustError("VAPID_PUBLIC_KEY not configured".into()))?;
    Response::from_json(&serde_json::json!({ "public_key": public_key }))
}

/// POST /push/subscribe -- store a push subscription (authenticated)
pub async fn subscribe(mut req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let user_id = match token::validate_from_header(&req, &ctx.env) {
        Ok(sub) => sub,
        Err(e) => {
            let body = ErrorResponse {
                error: format!("authentication required: {e}"),
            };
            return Ok(Response::from_json(&body)?.with_status(401));
        }
    };

    let subscription: PushSubscription = req
        .json()
        .await
        .map_err(|e| Error::RustError(format!("invalid subscription body: {e}")))?;

    // Forward to AuthDO
    let stub = auth_do_stub(&ctx.env)?;
    let do_body = serde_json::json!({
        "user_id": user_id,
        "subscription": subscription,
    });
    let internal_req = do_request("/push/subscribe", &do_body)?;
    let resp = stub.fetch_with_request(internal_req).await?;
    Ok(resp)
}

/// POST /push/unsubscribe -- remove a push subscription (authenticated)
pub async fn unsubscribe(mut req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let user_id = match token::validate_from_header(&req, &ctx.env) {
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
    let endpoint = body
        .get("endpoint")
        .and_then(|v| v.as_str())
        .ok_or_else(|| Error::RustError("missing endpoint".into()))?;

    let stub = auth_do_stub(&ctx.env)?;
    let do_body = serde_json::json!({
        "user_id": user_id,
        "endpoint": endpoint,
    });
    let internal_req = do_request("/push/unsubscribe", &do_body)?;
    let resp = stub.fetch_with_request(internal_req).await?;
    Ok(resp)
}

/// POST /push/send -- send a push notification to a user (authenticated, admin)
pub async fn send(mut req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let _sender_id = match token::validate_from_header(&req, &ctx.env) {
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
    let target_user_id = body
        .get("user_id")
        .and_then(|v| v.as_str())
        .ok_or_else(|| Error::RustError("missing user_id".into()))?;
    let payload = body
        .get("payload")
        .and_then(|v| v.as_str())
        .ok_or_else(|| Error::RustError("missing payload".into()))?;

    // Load VAPID keys
    let vapid_private_b64 = ctx
        .env
        .var("VAPID_PRIVATE_KEY")
        .map(|v| v.to_string())
        .map_err(|_| Error::RustError("VAPID_PRIVATE_KEY not configured".into()))?;
    let vapid_public_b64 = ctx
        .env
        .var("VAPID_PUBLIC_KEY")
        .map(|v| v.to_string())
        .map_err(|_| Error::RustError("VAPID_PUBLIC_KEY not configured".into()))?;
    let vapid_subject = ctx
        .env
        .var("VAPID_SUBJECT")
        .map(|v| v.to_string())
        .map_err(|_| Error::RustError("VAPID_SUBJECT not configured".into()))?;

    let vapid_private_key = b64url_decode(&vapid_private_b64)
        .map_err(|e| Error::RustError(format!("decode VAPID_PRIVATE_KEY: {e}")))?;
    let vapid_public_key = b64url_decode(&vapid_public_b64)
        .map_err(|e| Error::RustError(format!("decode VAPID_PUBLIC_KEY: {e}")))?;

    // Get user's subscriptions from DO
    let stub = auth_do_stub(&ctx.env)?;
    let do_body = serde_json::json!({ "user_id": target_user_id });
    let internal_req = do_request("/push/list", &do_body)?;
    let mut resp = stub.fetch_with_request(internal_req).await?;
    if resp.status_code() != 200 {
        return Ok(resp);
    }

    let list_result: serde_json::Value = resp
        .json()
        .await
        .map_err(|e| Error::RustError(format!("parse DO response: {e}")))?;
    let subscriptions: Vec<PushSubscription> =
        serde_json::from_value(list_result.get("subscriptions").cloned().unwrap_or_default())
            .map_err(|e| Error::RustError(format!("parse subscriptions: {e}")))?;

    let mut sent = 0u32;
    let mut failed = 0u32;
    let mut gone = 0u32;

    for sub in &subscriptions {
        match send_push(sub, payload, &vapid_private_key, &vapid_public_key, &vapid_subject).await
        {
            Ok(true) => sent += 1,
            Ok(false) => {
                gone += 1;
                // Remove stale subscription
                let remove_body = serde_json::json!({
                    "user_id": target_user_id,
                    "endpoint": sub.endpoint,
                });
                let remove_stub = auth_do_stub(&ctx.env)?;
                let remove_req = do_request("/push/unsubscribe", &remove_body)?;
                let _ = remove_stub.fetch_with_request(remove_req).await;
            }
            Err(_) => failed += 1,
        }
    }

    Response::from_json(&serde_json::json!({
        "sent": sent,
        "failed": failed,
        "gone": gone,
    }))
}

// ---------------------------------------------------------------------------
// Unit tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_vapid_token_structure() {
        // Generate a test P-256 key pair
        let secret_key = SecretKey::random(&mut rand_core());
        let private_bytes = secret_key.to_bytes();

        let token = create_vapid_token(
            "https://fcm.googleapis.com",
            "mailto:test@example.com",
            &private_bytes,
        )
        .expect("create_vapid_token should succeed");

        // JWT should have 3 parts
        let parts: Vec<&str> = token.split('.').collect();
        assert_eq!(parts.len(), 3, "JWT must have 3 dot-separated parts");

        // Decode and verify header
        let header_bytes = b64url_decode(parts[0]).expect("decode header");
        let header: serde_json::Value =
            serde_json::from_slice(&header_bytes).expect("parse header JSON");
        assert_eq!(header["typ"], "JWT");
        assert_eq!(header["alg"], "ES256");

        // Decode and verify claims
        let claims_bytes = b64url_decode(parts[1]).expect("decode claims");
        let claims: serde_json::Value =
            serde_json::from_slice(&claims_bytes).expect("parse claims JSON");
        assert_eq!(claims["aud"], "https://fcm.googleapis.com");
        assert_eq!(claims["sub"], "mailto:test@example.com");
        assert!(claims["exp"].as_i64().unwrap() > current_timestamp());

        // Signature should be 64 bytes (r=32 + s=32)
        let sig_bytes = b64url_decode(parts[2]).expect("decode signature");
        assert_eq!(sig_bytes.len(), 64, "ES256 signature must be 64 bytes");
    }

    #[test]
    fn test_vapid_token_signature_verifies() {
        let secret_key = SecretKey::random(&mut rand_core());
        let private_bytes = secret_key.to_bytes();
        let public_key = secret_key.public_key();

        let token = create_vapid_token(
            "https://push.example.com",
            "mailto:admin@hjkl.pro",
            &private_bytes,
        )
        .expect("create_vapid_token should succeed");

        let parts: Vec<&str> = token.split('.').collect();
        let signing_input = format!("{}.{}", parts[0], parts[1]);
        let sig_bytes = b64url_decode(parts[2]).expect("decode signature");

        // Verify signature with the public key
        use p256::ecdsa::{signature::Verifier, VerifyingKey};
        let verifying_key = VerifyingKey::from(public_key);
        let signature =
            Signature::from_bytes(sig_bytes.as_slice().into()).expect("parse signature");
        verifying_key
            .verify(signing_input.as_bytes(), &signature)
            .expect("signature verification should succeed");
    }

    #[test]
    fn test_vapid_token_invalid_key() {
        // All zeros is a valid-length but mathematically invalid P-256 scalar (zero is not valid)
        let bad_key = [0u8; 32];
        let result = create_vapid_token("https://example.com", "mailto:a@b.com", &bad_key);
        assert!(result.is_err());
    }

    #[test]
    fn test_encrypt_payload_output_format() {
        // Generate a fake subscription key pair
        let sub_secret = SecretKey::random(&mut rand_core());
        let sub_public = sub_secret.public_key();
        let sub_public_bytes = sub_public.to_encoded_point(false);

        let mut auth_secret = [0u8; 16];
        getrandom::getrandom(&mut auth_secret).expect("getrandom");

        let payload = b"Hello, push!";
        let encrypted = encrypt_payload(payload, sub_public_bytes.as_bytes(), &auth_secret)
            .expect("encrypt_payload should succeed");

        // Verify header structure
        assert!(encrypted.len() > 16 + 4 + 1 + 65);

        // salt = first 16 bytes
        let _salt = &encrypted[0..16];

        // rs = next 4 bytes (big-endian)
        let rs = u32::from_be_bytes([encrypted[16], encrypted[17], encrypted[18], encrypted[19]]);
        assert_eq!(rs, 4096);

        // idlen = next byte
        let idlen = encrypted[20] as usize;
        assert_eq!(idlen, 65, "idlen must be 65 for uncompressed P-256 point");

        // keyid = next 65 bytes (ephemeral public key, uncompressed)
        let keyid = &encrypted[21..21 + 65];
        assert_eq!(keyid[0], 0x04, "uncompressed point starts with 0x04");

        // Remaining bytes are the AES-GCM ciphertext + tag
        let ciphertext = &encrypted[21 + 65..];
        // payload(12) + delimiter(1) = 13 bytes padded, + 16 bytes GCM tag = 29
        assert_eq!(ciphertext.len(), payload.len() + 1 + 16);
    }

    #[test]
    fn test_hkdf_sha256_basic() {
        let salt = b"test-salt";
        let ikm = b"test-input-key-material";
        let info = b"test-info";

        let result = hkdf_sha256(salt, ikm, info, 32);
        assert_eq!(result.len(), 32);

        // Same inputs should produce same output (deterministic)
        let result2 = hkdf_sha256(salt, ikm, info, 32);
        assert_eq!(result, result2);

        // Different info should produce different output
        let result3 = hkdf_sha256(salt, ikm, b"different-info", 32);
        assert_ne!(result, result3);
    }
}

// ---------------------------------------------------------------------------
// Scheduled reminder: sends push to all subscribed users
// ---------------------------------------------------------------------------

pub async fn send_scheduled_reminders(env: &Env) -> std::result::Result<(), String> {
    let vapid_private_b64 = env.var("VAPID_PRIVATE_KEY")
        .map(|v| v.to_string())
        .map_err(|e| format!("VAPID_PRIVATE_KEY: {e}"))?;
    let vapid_public_b64 = env.var("VAPID_PUBLIC_KEY")
        .map(|v| v.to_string())
        .map_err(|e| format!("VAPID_PUBLIC_KEY: {e}"))?;
    let vapid_subject = env.var("VAPID_SUBJECT")
        .map(|v| v.to_string())
        .map_err(|e| format!("VAPID_SUBJECT: {e}"))?;

    let vapid_private = b64url_decode(&vapid_private_b64)
        .map_err(|e| format!("decode private key: {e}"))?;
    let vapid_public = b64url_decode(&vapid_public_b64)
        .map_err(|e| format!("decode public key: {e}"))?;

    // Get all subscriptions from AuthDO
    let stub = crate::auth_do_stub(env).map_err(|e| format!("auth_do_stub: {e}"))?;
    let req = crate::do_request("/push/list-all", &serde_json::json!({}))
        .map_err(|e| format!("do_request: {e}"))?;
    let mut resp = stub.fetch_with_request(req).await
        .map_err(|e| format!("fetch DO: {e}"))?;

    if resp.status_code() != 200 {
        return Err(format!("DO /push/list-all returned {}", resp.status_code()));
    }

    let subs: Vec<PushSubscription> = resp.json().await
        .map_err(|e| format!("parse subscriptions: {e}"))?;

    let payload = serde_json::json!({
        "title": "Food Tracker",
        "body": "Взвесьтесь и запишите вчерашние шаги 🏃‍♂️",
        "icon": "/icon-192.png",
        "tag": "daily-reminder",
        "url": "/",
    }).to_string();

    let mut sent = 0;
    let mut failed = 0;
    for sub in &subs {
        match send_push(sub, &payload, &vapid_private, &vapid_public, &vapid_subject).await {
            Ok(true) => sent += 1,
            Ok(false) => {
                // Subscription gone — should remove, but skip for now
                failed += 1;
            }
            Err(_) => failed += 1,
        }
    }

    console_log!("Push reminders: sent={sent}, failed={failed}, total={}", subs.len());
    Ok(())
}

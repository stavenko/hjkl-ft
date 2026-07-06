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

use crate::PushSubscription;

fn b64url_encode(data: &[u8]) -> String {
    URL_SAFE_NO_PAD.encode(data)
}

pub fn b64url_decode(s: &str) -> std::result::Result<Vec<u8>, base64::DecodeError> {
    URL_SAFE_NO_PAD.decode(s)
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

fn create_vapid_token(
    audience: &str,
    subject: &str,
    private_key: &[u8],
) -> std::result::Result<String, String> {
    let header = r#"{"typ":"JWT","alg":"ES256"}"#;
    let exp = current_timestamp() + 12 * 3600;
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

    let sig_bytes = signature.to_bytes();
    let sig_b64 = b64url_encode(&sig_bytes);

    Ok(format!("{signing_input}.{sig_b64}"))
}

fn hkdf_sha256(salt: &[u8], ikm: &[u8], info: &[u8], len: usize) -> Vec<u8> {
    let hk = Hkdf::<Sha256>::new(Some(salt), ikm);
    let mut okm = vec![0u8; len];
    hk.expand(info, &mut okm)
        .expect("HKDF expand failed");
    okm
}

fn encrypt_payload(
    payload: &[u8],
    subscription_key: &[u8],
    auth_secret: &[u8],
) -> std::result::Result<Vec<u8>, String> {
    let ephemeral_secret = SecretKey::random(&mut rand_core());
    let ephemeral_public = ephemeral_secret.public_key();
    let ephemeral_public_bytes = ephemeral_public.to_encoded_point(false);

    let sub_point = EncodedPoint::<p256::NistP256>::from_bytes(subscription_key)
        .map_err(|e| format!("invalid subscription key: {e}"))?;
    let sub_pubkey = PublicKey::from_encoded_point(&sub_point);
    let sub_pubkey: PublicKey = Option::from(sub_pubkey)
        .ok_or_else(|| "subscription key is not a valid P-256 point".to_string())?;

    let shared_secret = p256::ecdh::diffie_hellman(
        ephemeral_secret.to_nonzero_scalar(),
        sub_pubkey.as_affine(),
    );

    let mut auth_info = Vec::with_capacity(128);
    auth_info.extend_from_slice(b"WebPush: info\0");
    auth_info.extend_from_slice(subscription_key);
    auth_info.extend_from_slice(ephemeral_public_bytes.as_bytes());

    let ikm = hkdf_sha256(auth_secret, shared_secret.raw_secret_bytes(), &auth_info, 32);

    let mut salt = [0u8; 16];
    getrandom::getrandom(&mut salt).map_err(|e| format!("getrandom failed: {e}"))?;

    let cek_info = b"Content-Encoding: aes128gcm\0";
    let nonce_info = b"Content-Encoding: nonce\0";
    let key = hkdf_sha256(&salt, &ikm, cek_info, 16);
    let nonce_bytes = hkdf_sha256(&salt, &ikm, nonce_info, 12);

    let mut padded = Vec::with_capacity(payload.len() + 1);
    padded.extend_from_slice(payload);
    padded.push(0x02);

    let cipher = Aes128Gcm::new_from_slice(&key)
        .map_err(|e| format!("AES key init: {e}"))?;
    let nonce = Nonce::from_slice(&nonce_bytes);
    let ciphertext = cipher
        .encrypt(nonce, padded.as_ref())
        .map_err(|e| format!("AES-GCM encrypt: {e}"))?;

    let rs: u32 = 4096;
    let ephemeral_bytes = ephemeral_public_bytes.as_bytes();
    let idlen = ephemeral_bytes.len() as u8;

    let mut result = Vec::with_capacity(16 + 4 + 1 + ephemeral_bytes.len() + ciphertext.len());
    result.extend_from_slice(&salt);
    result.extend_from_slice(&rs.to_be_bytes());
    result.push(idlen);
    result.extend_from_slice(ephemeral_bytes);
    result.extend_from_slice(&ciphertext);

    Ok(result)
}

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

use elliptic_curve::rand_core as rand_core_crate;

pub async fn send_push(
    subscription: &PushSubscription,
    payload: &str,
    vapid_private_key: &[u8],
    vapid_public_key: &[u8],
    vapid_subject: &str,
) -> std::result::Result<bool, String> {
    let sub_key = b64url_decode(&subscription.keys.p256dh)
        .map_err(|e| format!("decode p256dh: {e}"))?;
    let auth_secret = b64url_decode(&subscription.keys.auth)
        .map_err(|e| format!("decode auth: {e}"))?;

    let encrypted = encrypt_payload(payload.as_bytes(), &sub_key, &auth_secret)?;

    let url = worker::Url::parse(&subscription.endpoint)
        .map_err(|e| format!("parse endpoint URL: {e}"))?;
    let audience = url.origin().ascii_serialization();

    let jwt = create_vapid_token(&audience, vapid_subject, vapid_private_key)?;
    let vapid_pub_b64 = b64url_encode(vapid_public_key);
    let authorization = format!("vapid t={jwt},k={vapid_pub_b64}");

    let headers = Headers::new();
    headers.set("Content-Type", "application/octet-stream")
        .map_err(|e| format!("set header: {e}"))?;
    headers.set("Content-Encoding", "aes128gcm")
        .map_err(|e| format!("set header: {e}"))?;
    headers.set("TTL", "86400")
        .map_err(|e| format!("set header: {e}"))?;
    headers.set("Authorization", &authorization)
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
        Ok(false)
    } else {
        Err(format!(
            "push service returned status {status} for endpoint {}",
            subscription.endpoint
        ))
    }
}

// Telegram WebApp initData validation.
//
// initData is the URL-encoded query string Telegram hands the Mini App. It carries a
// `hash` HMAC that proves the data came from Telegram for THIS bot. We validate it on
// every privileged /miniapp/* call and NEVER trust a user id from anywhere else.
//
// Algorithm (Telegram WebApp, confirmed):
//   data_check_string = all "key=value" pairs EXCEPT `hash` (and `signature`), sorted
//                       by key lexicographically, joined by '\n'.
//   secret_key = HMAC_SHA256(key="WebAppData", msg=bot_token)
//   computed   = hex_lower( HMAC_SHA256(key=secret_key, msg=data_check_string) )
//   valid      iff computed == hash  (constant-time compare)
// Plus an auth_date freshness check (reject older than 24h) to prevent replay.
//
// SECURITY: the raw initData and the derived secret_key are NEVER logged.

use hmac::{Hmac, Mac};
use sha2::Sha256;

type HmacSha256 = Hmac<Sha256>;

const MAX_AGE_SECONDS: i64 = 86_400; // 24h freshness window.

/// Validated identity extracted from a Telegram WebApp initData string.
pub struct InitDataOk {
    pub tg_user_id: i64,
}

/// Validate Telegram WebApp initData against the bot token. `now_ms` is the current
/// time in unix milliseconds (passed in so this fn stays pure/testable).
///
/// Returns Ok(InitDataOk{tg_user_id}) iff the HMAC matches AND auth_date is fresh.
/// Err(reason) on any failure — the caller maps every Err to 401 and does NOT log the
/// raw initData.
pub fn validate_init_data(
    init_data: &str,
    bot_token: &str,
    now_ms: i64,
) -> std::result::Result<InitDataOk, String> {
    // 1. Parse as a urlencoded query string → DECODED (key, value) pairs.
    let url = worker::Url::parse(&format!("https://x/?{init_data}"))
        .map_err(|e| format!("parse initData: {e}"))?;
    let pairs: Vec<(String, String)> = url
        .query_pairs()
        .map(|(k, v)| (k.into_owned(), v.into_owned()))
        .collect();

    // 2. Pull out `hash`.
    let provided_hash = pairs
        .iter()
        .find(|(k, _)| k == "hash")
        .map(|(_, v)| v.to_lowercase())
        .ok_or_else(|| "no hash".to_string())?;

    // 3. data_check_string: everything except `hash` (and `signature`), sorted by key.
    let mut check_pairs: Vec<&(String, String)> = pairs
        .iter()
        .filter(|(k, _)| k != "hash" && k != "signature")
        .collect();
    check_pairs.sort_by(|a, b| a.0.cmp(&b.0));
    let data_check_string = check_pairs
        .iter()
        .map(|(k, v)| format!("{k}={v}"))
        .collect::<Vec<_>>()
        .join("\n");

    // 4. secret_key = HMAC_SHA256(key="WebAppData", msg=bot_token).
    let mut mac = HmacSha256::new_from_slice(b"WebAppData")
        .map_err(|e| format!("hmac init: {e}"))?;
    mac.update(bot_token.as_bytes());
    let secret_key = mac.finalize().into_bytes();

    // 5. computed = hex_lower( HMAC_SHA256(key=secret_key, msg=data_check_string) ).
    let mut mac2 = HmacSha256::new_from_slice(&secret_key)
        .map_err(|e| format!("hmac init 2: {e}"))?;
    mac2.update(data_check_string.as_bytes());
    let computed_bytes = mac2.finalize().into_bytes();
    let mut computed = String::with_capacity(64);
    for b in computed_bytes {
        computed.push_str(&format!("{b:02x}"));
    }

    // 6. Constant-time compare.
    if !ct_eq(computed.as_bytes(), provided_hash.as_bytes()) {
        return Err("bad hash".to_string());
    }

    // 7. Freshness: auth_date is unix seconds.
    let auth_date = pairs
        .iter()
        .find(|(k, _)| k == "auth_date")
        .and_then(|(_, v)| v.parse::<i64>().ok())
        .ok_or_else(|| "no auth_date".to_string())?;
    let now_s = now_ms / 1000;
    if now_s - auth_date > MAX_AGE_SECONDS {
        return Err("stale".to_string());
    }

    // 8. Identity from the `user` JSON.
    let user_raw = pairs
        .iter()
        .find(|(k, _)| k == "user")
        .map(|(_, v)| v.as_str())
        .ok_or_else(|| "no user".to_string())?;
    let user: serde_json::Value =
        serde_json::from_str(user_raw).map_err(|e| format!("parse user: {e}"))?;
    let tg_user_id = user
        .get("id")
        .and_then(|v| v.as_i64())
        .ok_or_else(|| "no user.id".to_string())?;

    Ok(InitDataOk { tg_user_id })
}

/// Constant-time byte compare (length-check then XOR-accumulate).
fn ct_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff: u8 = 0;
    for (x, y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}

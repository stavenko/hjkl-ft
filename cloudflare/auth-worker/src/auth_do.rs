use async_trait::async_trait;
use base64::{engine::general_purpose::STANDARD as BASE64, Engine};
use hmac::{Hmac, Mac};
use passkey_server::error::Result as PkResult;
use passkey_server::types::{PasskeyState, StoredPasskey};
use passkey_server::{
    finish_login, finish_registration, start_login, start_registration, PasskeyConfig,
    PasskeyStore,
};
use serde::{Deserialize, Serialize};
use sha2::Sha256;
use std::cell::RefCell;
use wasm_bindgen::JsCast;
use worker::*;

use crate::types::{PairingSession, PairingStatus};

use crate::types::{TokenListResponse, TokenMetadata};

// Storage key prefixes for the global AuthDO
const STORAGE_KEY_CRED_PREFIX: &str = "cred:";
const STORAGE_KEY_USER_CREDS_PREFIX: &str = "user_creds:";
const STORAGE_KEY_USER_PREFIX: &str = "user:";
const STORAGE_KEY_PAIRING_PREFIX: &str = "pairing:";
const STORAGE_KEY_STATE_PREFIX: &str = "pk_state:";
const STORAGE_KEY_TOKEN_PREFIX: &str = "token:";
const STORAGE_KEY_USER_TOKENS_PREFIX: &str = "user_tokens:";
// Backup-phrase reverse index (normalized phrase → user_id) for username-less login,
// and a per-normalized-phrase fixed-window brute-force limiter.
const STORAGE_KEY_PHRASE_PREFIX: &str = "phrase:";
const STORAGE_KEY_PHRASE_RL_PREFIX: &str = "phrase_rl:";
// Failed phrase-login attempts allowed per normalized phrase per window before lockout.
const PHRASE_RL_MAX: i64 = 8;
const PHRASE_RL_WINDOW_MS: i64 = 3_600_000; // 1 hour
// Provider-agnostic identity reverse index: `identity:<provider>:<uid>` → user_id. Maps an
// external identity (Telegram today, others later) to one stable account. SEPARATE from
// passkey/phrase. Login codes (bearer of a session for a user) live per-user with a TTL,
// a per-user cooldown on issuance, and an attempt cap on verification.
const STORAGE_KEY_IDENTITY_PREFIX: &str = "identity:";
// Login codes are keyed by their HASH (`codehash:<hash>` → {userId, expires}) — NOT one per
// user — so several outstanding codes (e.g. the Mini App mints one per /miniapp/me while a
// prior one is still in the onboarding link) are ALL valid until consumed/expired. No overwrite.
const STORAGE_KEY_CODEHASH_PREFIX: &str = "codehash:";
const STORAGE_KEY_CODE_COOLDOWN_PREFIX: &str = "code_cd:";
const CODE_TTL_MS: i64 = 600_000; // 10 min
const CODE_COOLDOWN_MS: i64 = 60_000; // 1 min between user-triggered sends
// Global fixed-window limiter on verify (blunts brute force of the 6-digit space).
const STORAGE_KEY_VERIFY_RL: &str = "code_verify_rl";
const VERIFY_RL_MAX: i64 = 30;
const VERIFY_RL_WINDOW_MS: i64 = 600_000;
/// Story chapters that became AVAILABLE in the running app for a user (chapter_id → first-seen
/// ms), stored NEXT TO the persona (keyed by user_id). Reported from the app UI. A non-empty map
/// means the first chapter unlocked → the user «entered the system» (the Mini App access signal).
const STORAGE_KEY_CHAPTERS_PREFIX: &str = "chapters:";

type HmacSha256 = Hmac<Sha256>;

/// Inputs are the FIXED env-derived configs plus the ceremony origin.
/// Returns true => use ADMIN config, false => use APP config.
/// Selection is pure string equality against the configured admin origin;
/// a client-supplied origin is ONLY ever used as a selector, never copied
/// into the returned config.
fn select_is_admin(origin: &str, admin_rp_origin: &str) -> bool {
    !origin.is_empty() && origin == admin_rp_origin
}

// ---- Recovery hash helpers (HMAC-SHA256 based) ----

#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct RecoveryHashData {
    pub salt_b64: String,
    pub hash_b64: String,
}

/// A linked external identity / delivery channel. Provider-agnostic: Telegram is the first,
/// email/etc. slot in later without touching the core account. `provider_uid` is a string so
/// any provider's id shape fits. This is how we (a) map a provider account → our user_id and
/// (b) know where to deliver a login code.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct Identity {
    provider: String,
    provider_uid: String,
    #[serde(default)]
    username: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
struct UserMetadata {
    recovery_hash_data: Option<RecoveryHashData>,
    created_at: i64,
    /// Plaintext backup phrase (user explicit: NO hash — it must be re-showable in
    /// Settings). Paired with the `phrase:{normalized}` → user_id reverse index so login
    /// needs only the phrase. `serde(default)` so pre-existing records deserialize.
    #[serde(default)]
    recovery_phrase: Option<String>,
    /// Linked external identity (e.g. Telegram) — resolved at first touch, used for code
    /// delivery + admin ("who paid but hasn't set up access"). `serde(default)` for old rows.
    #[serde(default)]
    identity: Option<Identity>,
}

/// Canonical form of a backup phrase for storage/lookup: lowercased, whitespace
/// collapsed to single spaces, trimmed. Both `set` and `resolve` normalize identically
/// so the reverse index round-trips regardless of the user's spacing/casing.
fn normalize_phrase(phrase: &str) -> String {
    phrase.split_whitespace().collect::<Vec<_>>().join(" ").to_lowercase()
}

fn compute_recovery_hmac(recovery_key: &str, salt: &[u8]) -> Vec<u8> {
    let mut mac = HmacSha256::new_from_slice(salt).expect("HMAC accepts any key size");
    mac.update(recovery_key.as_bytes());
    mac.finalize().into_bytes().to_vec()
}

pub(crate) fn create_recovery_hash(recovery_key: &str) -> RecoveryHashData {
    let mut salt = [0u8; 32];
    getrandom::getrandom(&mut salt).expect("getrandom failed");
    let salt_b64 = BASE64.encode(salt);
    let hash_bytes = compute_recovery_hmac(recovery_key, &salt);
    let hash_b64 = BASE64.encode(hash_bytes);
    RecoveryHashData { salt_b64, hash_b64 }
}

pub(crate) fn verify_recovery_key(recovery_key: &str, data: &RecoveryHashData) -> Result<bool> {
    let salt = BASE64
        .decode(&data.salt_b64)
        .map_err(|e| Error::RustError(format!("decode salt: {e}")))?;
    let stored_hash = BASE64
        .decode(&data.hash_b64)
        .map_err(|e| Error::RustError(format!("decode hash: {e}")))?;

    let mut mac = HmacSha256::new_from_slice(&salt)
        .map_err(|e| Error::RustError(format!("hmac init: {e}")))?;
    mac.update(recovery_key.as_bytes());

    Ok(mac.verify_slice(&stored_hash).is_ok())
}

// ---- PasskeyStore backed by DO storage, using RefCell for interior mutability ----

struct DoPasskeyStore {
    credentials: RefCell<Vec<StoredPasskey>>,
    states: RefCell<Vec<(String, PasskeyState)>>,
}

impl DoPasskeyStore {
    fn new(credentials: Vec<StoredPasskey>, states: Vec<(String, PasskeyState)>) -> Self {
        Self {
            credentials: RefCell::new(credentials),
            states: RefCell::new(states),
        }
    }
}

fn now_ms() -> i64 {
    Date::now().as_millis() as i64
}

#[async_trait(?Send)]
impl PasskeyStore for DoPasskeyStore {
    async fn create_passkey(
        &self,
        user_id: String,
        cred_id: &str,
        public_key: &str,
        name: &str,
        counter: i64,
        created_at: i64,
    ) -> PkResult<()> {
        self.credentials.borrow_mut().push(StoredPasskey {
            user_id,
            cred_id: cred_id.to_string(),
            public_key: public_key.to_string(),
            name: name.to_string(),
            created_at,
            last_used_at: created_at,
            counter,
        });
        Ok(())
    }

    async fn get_passkey(&self, cred_id: &str) -> PkResult<Option<StoredPasskey>> {
        Ok(self
            .credentials
            .borrow()
            .iter()
            .find(|c| c.cred_id == cred_id)
            .cloned())
    }

    async fn list_passkeys(&self, user_id: String) -> PkResult<Vec<StoredPasskey>> {
        Ok(self
            .credentials
            .borrow()
            .iter()
            .filter(|c| c.user_id == user_id)
            .cloned()
            .collect())
    }

    async fn delete_passkey(&self, user_id: String, cred_id: &str) -> PkResult<()> {
        self.credentials
            .borrow_mut()
            .retain(|c| !(c.cred_id == cred_id && c.user_id == user_id));
        Ok(())
    }

    async fn update_passkey_counter(
        &self,
        cred_id: &str,
        new_counter: i64,
        last_used_at: i64,
    ) -> PkResult<()> {
        let mut creds = self.credentials.borrow_mut();
        if let Some(pk) = creds.iter_mut().find(|c| c.cred_id == cred_id) {
            pk.counter = new_counter;
            pk.last_used_at = last_used_at;
        }
        Ok(())
    }

    async fn update_passkey_name(&self, cred_id: &str, new_name: &str) -> PkResult<()> {
        let mut creds = self.credentials.borrow_mut();
        if let Some(pk) = creds.iter_mut().find(|c| c.cred_id == cred_id) {
            pk.name = new_name.to_string();
        }
        Ok(())
    }

    async fn save_state(&self, id: &str, state_json: &str, expires_at: i64) -> PkResult<()> {
        let state = PasskeyState {
            id: id.to_string(),
            state_json: state_json.to_string(),
            expires_at,
        };
        let mut states = self.states.borrow_mut();
        if let Some(existing) = states.iter_mut().find(|(k, _)| k == id) {
            existing.1 = state;
        } else {
            states.push((id.to_string(), state));
        }
        Ok(())
    }

    async fn get_state(&self, id: &str) -> PkResult<Option<PasskeyState>> {
        let now = now_ms();
        Ok(self
            .states
            .borrow()
            .iter()
            .find(|(k, s)| k == id && s.expires_at > now)
            .map(|(_, s)| s.clone()))
    }

    async fn delete_state(&self, id: &str) -> PkResult<()> {
        self.states.borrow_mut().retain(|(k, _)| k != id);
        Ok(())
    }
}

// ---- Durable Object ----

#[durable_object]
pub struct AuthDO {
    state: worker::durable::State,
    env: Env,
}

impl AuthDO {
    /// The fixed app RP_ORIGIN env value (used to resolve app-only flows like
    /// pairing to the app config). Falls back to https://{RP_ID} like the
    /// original config logic.
    fn app_rp_origin(&self) -> String {
        self.env
            .var("RP_ORIGIN")
            .map(|v| v.to_string())
            .unwrap_or_else(|_| {
                format!(
                    "https://{}",
                    self.env
                        .var("RP_ID")
                        .map(|v| v.to_string())
                        .unwrap_or_default()
                )
            })
    }

    fn passkey_config(&self, origin: &str) -> Result<PasskeyConfig> {
        // Read all four FIXED env values up front (vars only, never the client origin).
        let app_rp_id = self
            .env
            .var("RP_ID")
            .map(|v| v.to_string())
            .map_err(|_| Error::RustError("RP_ID not configured".into()))?;
        let app_rp_origin = self
            .env
            .var("RP_ORIGIN")
            .map(|v| v.to_string())
            .unwrap_or_else(|_| format!("https://{app_rp_id}"));
        let admin_rp_id = self
            .env
            .var("ADMIN_RP_ID")
            .map(|v| v.to_string())
            .map_err(|_| Error::RustError("ADMIN_RP_ID not configured".into()))?;
        let admin_rp_origin = self
            .env
            .var("ADMIN_RP_ORIGIN")
            .map(|v| v.to_string())
            .unwrap_or_else(|_| format!("https://{admin_rp_id}"));

        // Fail loudly on empty origin: do NOT silently fall back to a scope.
        if origin.is_empty() {
            return Err(Error::RustError("missing ceremony origin".into()));
        }

        let (rp_id, rp_origin) = if select_is_admin(origin, &admin_rp_origin) {
            (admin_rp_id, admin_rp_origin)
        } else {
            // App path selects EXACTLY the existing app config — no behavior change,
            // no downgrade, and the client origin is discarded here.
            (app_rp_id, app_rp_origin)
        };

        Ok(PasskeyConfig {
            rp_id,
            rp_name: "Food Tracker".to_string(),
            origin: rp_origin, // FIXED env value, never the client-supplied origin
            state_ttl: 300,
        })
    }

    // ---- Storage helpers ----

    async fn load_all_credentials(&self) -> Result<Vec<StoredPasskey>> {
        let map = self
            .state
            .storage()
            .list_with_options(
                worker::durable::ListOptions::new().prefix(STORAGE_KEY_CRED_PREFIX),
            )
            .await?;
        let mut result = Vec::new();
        let iter = js_sys::try_iter(&map).ok().flatten();
        if let Some(iter) = iter {
            for entry in iter {
                let entry = entry.map_err(Error::from)?;
                let arr: js_sys::Array = entry.unchecked_into();
                let val = arr.get(1);
                if let Ok(json_str) = serde_wasm_bindgen::from_value::<String>(val) {
                    let cred: StoredPasskey = serde_json::from_str(&json_str)
                        .map_err(|e| Error::RustError(format!("parse credential: {e}")))?;
                    result.push(cred);
                }
            }
        }
        Ok(result)
    }

    async fn load_user_credentials(&self, user_id: &str) -> Result<Vec<StoredPasskey>> {
        let cred_ids = self.load_user_cred_ids(user_id).await?;
        let mut result = Vec::new();
        for cred_id in &cred_ids {
            let key = format!("{STORAGE_KEY_CRED_PREFIX}{cred_id}");
            let stored: Option<String> = self.state.storage().get(&key).await?;
            if let Some(json) = stored {
                let cred: StoredPasskey = serde_json::from_str(&json)
                    .map_err(|e| Error::RustError(format!("parse credential: {e}")))?;
                result.push(cred);
            }
        }
        Ok(result)
    }

    async fn load_user_cred_ids(&self, user_id: &str) -> Result<Vec<String>> {
        let key = format!("{STORAGE_KEY_USER_CREDS_PREFIX}{user_id}");
        let stored: Option<String> = self.state.storage().get(&key).await?;
        match stored {
            Some(json) => serde_json::from_str(&json)
                .map_err(|e| Error::RustError(format!("parse user_creds: {e}"))),
            None => Ok(Vec::new()),
        }
    }

    async fn save_credential(&self, cred: &StoredPasskey) -> Result<()> {
        let cred_key = format!("{STORAGE_KEY_CRED_PREFIX}{}", cred.cred_id);
        let cred_json = serde_json::to_string(cred)
            .map_err(|e| Error::RustError(format!("serialize credential: {e}")))?;
        self.state.storage().put(&cred_key, cred_json).await?;

        // Append to user_creds list
        let mut cred_ids = self.load_user_cred_ids(&cred.user_id).await?;
        if !cred_ids.contains(&cred.cred_id) {
            cred_ids.push(cred.cred_id.clone());
            let list_key = format!("{STORAGE_KEY_USER_CREDS_PREFIX}{}", cred.user_id);
            let list_json = serde_json::to_string(&cred_ids)
                .map_err(|e| Error::RustError(format!("serialize cred list: {e}")))?;
            self.state.storage().put(&list_key, list_json).await?;
        }
        Ok(())
    }

    async fn load_states(&self) -> Result<Vec<(String, PasskeyState)>> {
        let map = self
            .state
            .storage()
            .list_with_options(
                worker::durable::ListOptions::new().prefix(STORAGE_KEY_STATE_PREFIX),
            )
            .await?;
        let now = now_ms();
        let mut result = Vec::new();
        let iter = js_sys::try_iter(&map).ok().flatten();
        if let Some(iter) = iter {
            for entry in iter {
                let entry = entry.map_err(Error::from)?;
                let arr: js_sys::Array = entry.unchecked_into();
                let key: String = arr.get(0).as_string().unwrap_or_default();
                let val = arr.get(1);
                if let Ok(json_str) = serde_wasm_bindgen::from_value::<String>(val) {
                    if let Ok(state) = serde_json::from_str::<PasskeyState>(&json_str) {
                        if state.expires_at > now {
                            let logical_id = key
                                .strip_prefix(STORAGE_KEY_STATE_PREFIX)
                                .unwrap_or(&key)
                                .to_string();
                            result.push((logical_id, state));
                        }
                    }
                }
            }
        }
        Ok(result)
    }

    async fn flush_store(&self, store: &DoPasskeyStore) -> Result<()> {
        let storage = self.state.storage();

        // Persist each credential individually under cred:{cred_id}
        // and update user_creds:{user_id} lists
        let creds = store.credentials.borrow();
        // Group by user_id to update cred lists
        let mut user_cred_map: std::collections::HashMap<String, Vec<String>> =
            std::collections::HashMap::new();
        for cred in creds.iter() {
            let cred_key = format!("{STORAGE_KEY_CRED_PREFIX}{}", cred.cred_id);
            let cred_json = serde_json::to_string(cred)
                .map_err(|e| Error::RustError(format!("serialize credential: {e}")))?;
            storage.put(&cred_key, cred_json).await?;
            user_cred_map
                .entry(cred.user_id.clone())
                .or_default()
                .push(cred.cred_id.clone());
        }
        for (user_id, cred_ids) in &user_cred_map {
            let list_key = format!("{STORAGE_KEY_USER_CREDS_PREFIX}{user_id}");
            let list_json = serde_json::to_string(cred_ids)
                .map_err(|e| Error::RustError(format!("serialize cred list: {e}")))?;
            storage.put(&list_key, list_json).await?;
        }

        // Delete all existing state keys
        let existing_map = storage
            .list_with_options(
                worker::durable::ListOptions::new().prefix(STORAGE_KEY_STATE_PREFIX),
            )
            .await?;
        let mut keys_to_delete: Vec<String> = Vec::new();
        let iter = js_sys::try_iter(&existing_map).ok().flatten();
        if let Some(iter) = iter {
            for entry in iter {
                let entry = entry.map_err(Error::from)?;
                let arr: js_sys::Array = entry.unchecked_into();
                if let Some(key) = arr.get(0).as_string() {
                    keys_to_delete.push(key);
                }
            }
        }
        if !keys_to_delete.is_empty() {
            storage.delete_multiple(keys_to_delete).await?;
        }

        // Write current states as JSON strings
        let states = store.states.borrow();
        for (id, state) in states.iter() {
            let storage_key = format!("{STORAGE_KEY_STATE_PREFIX}{id}");
            let state_json = serde_json::to_string(state)
                .map_err(|e| Error::RustError(format!("serialize state: {e}")))?;
            storage.put(&storage_key, state_json).await?;
        }
        Ok(())
    }

    async fn load_user_metadata(&self, user_id: &str) -> Result<Option<UserMetadata>> {
        let key = format!("{STORAGE_KEY_USER_PREFIX}{user_id}");
        let stored: Option<String> = self.state.storage().get(&key).await?;
        match stored {
            Some(json) => {
                let meta: UserMetadata = serde_json::from_str(&json)
                    .map_err(|e| Error::RustError(format!("parse user metadata: {e}")))?;
                Ok(Some(meta))
            }
            None => Ok(None),
        }
    }

    async fn save_user_metadata(&self, user_id: &str, meta: &UserMetadata) -> Result<()> {
        let key = format!("{STORAGE_KEY_USER_PREFIX}{user_id}");
        let json = serde_json::to_string(meta)
            .map_err(|e| Error::RustError(format!("serialize user metadata: {e}")))?;
        self.state.storage().put(&key, json).await?;
        Ok(())
    }

    // ---- Registration handlers ----

    async fn handle_register_begin(&self, user_id: Option<&str>, display_name: Option<&str>, origin: &str) -> Result<Response> {
        let user_id = match user_id {
            Some(id) if !id.is_empty() => id.to_string(),
            _ => uuid::Uuid::new_v4().to_string(),
        };

        let display_name = match display_name {
            Some(n) if !n.trim().is_empty() => n.trim().to_string(),
            _ => user_id.clone(),
        };

        let config = self.passkey_config(origin)?;
        let credentials = self.load_user_credentials(&user_id).await?;
        let states = self.load_states().await?;
        let store = DoPasskeyStore::new(credentials, states);

        // Ensure user metadata exists
        if self.load_user_metadata(&user_id).await?.is_none() {
            let meta = UserMetadata {
                recovery_hash_data: None,
                created_at: now_ms(),
                recovery_phrase: None,
                identity: None,
            };
            self.save_user_metadata(&user_id, &meta).await?;
        }

        let options =
            start_registration(&store, &user_id, &display_name, &display_name, &config, now_ms())
                .await
                .map_err(|e| Error::RustError(format!("registration begin: {e}")))?;

        self.flush_store(&store).await?;

        let body = serde_json::json!({ "publicKey": options, "user_id": user_id });
        Response::from_json(&body)
    }

    async fn handle_register_finish(&self, credential: serde_json::Value, user_id: &str, origin: &str) -> Result<Response> {
        let config = self.passkey_config(origin)?;
        let credentials = self.load_user_credentials(user_id).await?;
        let states = self.load_states().await?;
        let store = DoPasskeyStore::new(credentials, states);

        let response: passkey_server::types::RegistrationResponse =
            serde_json::from_value(credential)
                .map_err(|e| Error::RustError(format!("parse credential: {e}")))?;

        finish_registration(&store, user_id, &config, response, now_ms())
            .await
            .map_err(|e| Error::RustError(format!("registration finish: {e}")))?;

        self.flush_store(&store).await?;

        // Also persist each new credential individually
        let creds = store.credentials.borrow();
        for cred in creds.iter() {
            self.save_credential(cred).await?;
        }

        Response::from_json(&serde_json::json!({ "ok": true, "user_id": user_id }))
    }

    // ---- Authentication handlers (discoverable, no username) ----

    async fn handle_authenticate_begin(&self, origin: &str) -> Result<Response> {
        let config = self.passkey_config(origin)?;
        // Load ALL credentials for discoverable auth
        let credentials = self.load_all_credentials().await?;
        let states = self.load_states().await?;
        let store = DoPasskeyStore::new(credentials, states);

        let options = start_login(&store, &config, now_ms())
            .await
            .map_err(|e| Error::RustError(format!("login begin: {e}")))?;

        self.flush_store(&store).await?;

        let body = serde_json::json!({ "publicKey": options });
        Response::from_json(&body)
    }

    async fn handle_authenticate_finish(
        &self,
        credential: serde_json::Value,
        origin: &str,
    ) -> Result<Response> {
        let config = self.passkey_config(origin)?;
        // Load ALL credentials so we can look up by credential ID
        let credentials = self.load_all_credentials().await?;
        let states = self.load_states().await?;
        let store = DoPasskeyStore::new(credentials, states);

        let response: passkey_server::types::LoginResponse =
            serde_json::from_value(credential)
                .map_err(|e| Error::RustError(format!("parse credential: {e}")))?;

        let user_id = finish_login(&store, &config, response, now_ms())
            .await
            .map_err(|e| Error::RustError(format!("login finish: {e}")))?;

        self.flush_store(&store).await?;

        // Also update individual credential storage (counter may have changed)
        let creds = store.credentials.borrow();
        for cred in creds.iter() {
            let cred_key = format!("{STORAGE_KEY_CRED_PREFIX}{}", cred.cred_id);
            let cred_json = serde_json::to_string(cred)
                .map_err(|e| Error::RustError(format!("serialize credential: {e}")))?;
            self.state.storage().put(&cred_key, cred_json).await?;
        }

        Response::from_json(&serde_json::json!({ "ok": true, "user_id": user_id }))
    }

    // ---- Pairing handlers ----

    fn pairing_storage_key(pairing_id: &str) -> String {
        format!("{STORAGE_KEY_PAIRING_PREFIX}{pairing_id}")
    }

    async fn handle_pair_create(
        &self,
        pairing_id: &str,
        secret_hash: &str,
        user_id: &str,
        created_at: i64,
        expires_at: i64,
    ) -> Result<Response> {
        let session = PairingSession {
            pairing_id: pairing_id.to_string(),
            secret_hash: secret_hash.to_string(),
            user_id: user_id.to_string(),
            created_at,
            expires_at,
            status: PairingStatus::Pending,
        };
        let json = serde_json::to_string(&session)
            .map_err(|e| Error::RustError(format!("serialize pairing session: {e}")))?;
        self.state
            .storage()
            .put(&Self::pairing_storage_key(pairing_id), json)
            .await?;
        Response::from_json(&serde_json::json!({ "ok": true }))
    }

    async fn handle_pair_request(
        &self,
        pairing_id: &str,
        secret_hash: &str,
        created_at: i64,
        expires_at: i64,
    ) -> Result<Response> {
        // Same as create but no user_id yet
        let session = PairingSession {
            pairing_id: pairing_id.to_string(),
            secret_hash: secret_hash.to_string(),
            user_id: String::new(),
            created_at,
            expires_at,
            status: PairingStatus::Pending,
        };
        let json = serde_json::to_string(&session)
            .map_err(|e| Error::RustError(format!("serialize pairing session: {e}")))?;
        self.state
            .storage()
            .put(&Self::pairing_storage_key(pairing_id), json)
            .await?;
        Response::from_json(&serde_json::json!({ "ok": true }))
    }

    async fn handle_pair_approve(
        &self,
        pairing_id: &str,
        secret_hash: &str,
        user_id: &str,
    ) -> Result<Response> {
        let key = Self::pairing_storage_key(pairing_id);
        let stored: Option<String> = self.state.storage().get(&key).await?;
        let json_str =
            stored.ok_or_else(|| Error::RustError("pairing session not found".into()))?;
        let mut session: PairingSession = serde_json::from_str(&json_str)
            .map_err(|e| Error::RustError(format!("parse pairing session: {e}")))?;

        let now = now_ms() / 1000;
        if session.expires_at < now {
            session.status = PairingStatus::Expired;
            let updated = serde_json::to_string(&session)
                .map_err(|e| Error::RustError(format!("serialize: {e}")))?;
            self.state.storage().put(&key, updated).await?;
            return Response::error("pairing session expired", 410);
        }
        if session.status != PairingStatus::Pending {
            return Response::error("pairing session already used", 409);
        }
        if session.secret_hash != secret_hash {
            return Response::error("invalid secret", 403);
        }

        // Bind user to session and mark as approved
        session.user_id = user_id.to_string();
        session.status = PairingStatus::Approved;
        let updated = serde_json::to_string(&session)
            .map_err(|e| Error::RustError(format!("serialize: {e}")))?;
        self.state.storage().put(&key, updated).await?;

        Response::from_json(&serde_json::json!({ "ok": true }))
    }

    async fn handle_pair_claim(
        &self,
        pairing_id: &str,
        secret_hash: &str,
    ) -> Result<Response> {
        let key = Self::pairing_storage_key(pairing_id);
        let stored: Option<String> = self.state.storage().get(&key).await?;
        let json_str =
            stored.ok_or_else(|| Error::RustError("pairing session not found".into()))?;
        let mut session: PairingSession = serde_json::from_str(&json_str)
            .map_err(|e| Error::RustError(format!("parse pairing session: {e}")))?;

        let now = now_ms() / 1000;
        if session.expires_at < now {
            session.status = PairingStatus::Expired;
            let updated = serde_json::to_string(&session)
                .map_err(|e| Error::RustError(format!("serialize: {e}")))?;
            self.state.storage().put(&key, updated).await?;
            return Response::error("pairing session expired", 410);
        }
        if session.status != PairingStatus::Pending && session.status != PairingStatus::Approved {
            return Response::error(
                format!(
                    "pairing session already {}",
                    serde_json::to_string(&session.status).unwrap_or_default()
                ),
                409,
            );
        }
        if session.secret_hash != secret_hash {
            return Response::error("invalid secret", 403);
        }

        // Must have a user_id bound (from create or approve)
        if session.user_id.is_empty() {
            return Response::error("pairing session not yet approved", 409);
        }

        session.status = PairingStatus::Claimed;
        let updated = serde_json::to_string(&session)
            .map_err(|e| Error::RustError(format!("serialize: {e}")))?;
        self.state.storage().put(&key, updated).await?;

        // Start passkey registration for the bound user
        let user_id = session.user_id.clone();
        // For pairing, load display_name from user metadata if available
        let dn = match self.load_user_metadata(&user_id).await? {
            Some(_) => user_id.clone(), // existing user, use user_id as fallback
            None => user_id.clone(),
        };
        // Pairing is an app-only flow; resolve to the app config explicitly.
        let config = self.passkey_config(&self.app_rp_origin())?;
        let credentials = self.load_user_credentials(&user_id).await?;
        let states = self.load_states().await?;
        let store = DoPasskeyStore::new(credentials, states);

        let options =
            start_registration(&store, &user_id, &dn, &dn, &config, now_ms())
                .await
                .map_err(|e| Error::RustError(format!("registration begin: {e}")))?;

        self.flush_store(&store).await?;

        let body = serde_json::json!({ "publicKey": options, "user_id": user_id });
        Response::from_json(&body)
    }

    async fn handle_pair_finish(
        &self,
        pairing_id: &str,
        credential: serde_json::Value,
    ) -> Result<Response> {
        let key = Self::pairing_storage_key(pairing_id);
        let stored: Option<String> = self.state.storage().get(&key).await?;
        let json_str =
            stored.ok_or_else(|| Error::RustError("pairing session not found".into()))?;
        let mut session: PairingSession = serde_json::from_str(&json_str)
            .map_err(|e| Error::RustError(format!("parse pairing session: {e}")))?;

        if session.status != PairingStatus::Claimed {
            return Response::error("pairing session not in claimed state", 409);
        }

        let user_id = session.user_id.clone();

        // Complete passkey registration. Pairing is an app-only flow.
        let config = self.passkey_config(&self.app_rp_origin())?;
        let credentials = self.load_user_credentials(&user_id).await?;
        let states = self.load_states().await?;
        let store = DoPasskeyStore::new(credentials, states);

        let response: passkey_server::types::RegistrationResponse =
            serde_json::from_value(credential)
                .map_err(|e| Error::RustError(format!("parse credential: {e}")))?;

        finish_registration(&store, &user_id, &config, response, now_ms())
            .await
            .map_err(|e| Error::RustError(format!("registration finish: {e}")))?;

        self.flush_store(&store).await?;

        // Persist credentials individually
        let creds = store.credentials.borrow();
        for cred in creds.iter() {
            self.save_credential(cred).await?;
        }

        // Mark pairing as completed
        session.status = PairingStatus::Completed;
        let updated = serde_json::to_string(&session)
            .map_err(|e| Error::RustError(format!("serialize: {e}")))?;
        self.state.storage().put(&key, updated).await?;

        Response::from_json(&serde_json::json!({ "ok": true, "user_id": user_id }))
    }

    async fn handle_pair_status(&self, pairing_id: &str) -> Result<Response> {
        let key = Self::pairing_storage_key(pairing_id);
        let stored: Option<String> = self.state.storage().get(&key).await?;
        match stored {
            Some(json) => {
                let mut session: PairingSession = serde_json::from_str(&json)
                    .map_err(|e| Error::RustError(format!("parse pairing session: {e}")))?;
                let now = now_ms() / 1000;
                if session.status == PairingStatus::Pending && session.expires_at < now {
                    session.status = PairingStatus::Expired;
                }
                Response::from_json(&serde_json::json!({ "status": session.status }))
            }
            None => Response::error("pairing session not found", 404),
        }
    }

    // ---- Token storage handlers ----

    async fn handle_token_store(
        &self,
        token_id: &str,
        user_id: &str,
        fingerprint: &str,
        created_at: i64,
    ) -> Result<Response> {
        let meta = TokenMetadata {
            token_id: token_id.to_string(),
            user_id: user_id.to_string(),
            fingerprint: fingerprint.to_string(),
            created_at,
            last_used_at: created_at,
        };
        let meta_json = serde_json::to_string(&meta)
            .map_err(|e| Error::RustError(format!("serialize token metadata: {e}")))?;
        let token_key = format!("{STORAGE_KEY_TOKEN_PREFIX}{token_id}");
        self.state.storage().put(&token_key, meta_json).await?;

        // Append token_id to user's token list
        let list_key = format!("{STORAGE_KEY_USER_TOKENS_PREFIX}{user_id}");
        let stored: Option<String> = self.state.storage().get(&list_key).await?;
        let mut token_ids: Vec<String> = match stored {
            Some(json) => serde_json::from_str(&json)
                .map_err(|e| Error::RustError(format!("parse user_tokens: {e}")))?,
            None => Vec::new(),
        };
        if !token_ids.contains(&token_id.to_string()) {
            token_ids.push(token_id.to_string());
            let list_json = serde_json::to_string(&token_ids)
                .map_err(|e| Error::RustError(format!("serialize token list: {e}")))?;
            self.state.storage().put(&list_key, list_json).await?;
        }

        Response::from_json(&serde_json::json!({ "ok": true }))
    }

    async fn handle_token_validate(&self, token_id: &str) -> Result<Response> {
        let token_key = format!("{STORAGE_KEY_TOKEN_PREFIX}{token_id}");
        let stored: Option<String> = self.state.storage().get(&token_key).await?;
        match stored {
            Some(json) => {
                // Update last_used_at
                let mut meta: TokenMetadata = serde_json::from_str(&json)
                    .map_err(|e| Error::RustError(format!("parse token metadata: {e}")))?;
                meta.last_used_at = now_ms() / 1000;
                let updated = serde_json::to_string(&meta)
                    .map_err(|e| Error::RustError(format!("serialize token metadata: {e}")))?;
                self.state.storage().put(&token_key, updated).await?;
                Response::from_json(&serde_json::json!({ "ok": true }))
            }
            None => Response::error("token not found", 404),
        }
    }

    async fn handle_token_list(&self, user_id: &str) -> Result<Response> {
        let list_key = format!("{STORAGE_KEY_USER_TOKENS_PREFIX}{user_id}");
        let stored: Option<String> = self.state.storage().get(&list_key).await?;
        let token_ids: Vec<String> = match stored {
            Some(json) => serde_json::from_str(&json)
                .map_err(|e| Error::RustError(format!("parse user_tokens: {e}")))?,
            None => Vec::new(),
        };

        let mut tokens = Vec::new();
        for tid in &token_ids {
            let token_key = format!("{STORAGE_KEY_TOKEN_PREFIX}{tid}");
            let stored: Option<String> = self.state.storage().get(&token_key).await?;
            if let Some(json) = stored {
                if let Ok(meta) = serde_json::from_str::<TokenMetadata>(&json) {
                    tokens.push(meta);
                }
            }
        }

        let resp = TokenListResponse { tokens };
        Response::from_json(&resp)
    }

    // ---- Recovery handlers ----

    async fn handle_recovery_set(
        &self,
        user_id: &str,
        hash_data: RecoveryHashData,
    ) -> Result<Response> {
        let mut meta = self
            .load_user_metadata(user_id)
            .await?
            .unwrap_or_else(|| UserMetadata {
                recovery_hash_data: None,
                created_at: now_ms(),
                recovery_phrase: None,
                identity: None,
            });
        meta.recovery_hash_data = Some(hash_data);
        self.save_user_metadata(user_id, &meta).await?;
        Response::from_json(&serde_json::json!({ "status": "ok" }))
    }

    // ---- Backup-phrase handlers (plaintext + reverse index; username-less login) ----

    /// Set/replace the user's backup phrase. Normalizes, rejects an empty/too-short
    /// phrase, and enforces global uniqueness via the reverse index (a collision with
    /// ANOTHER user → `{status:"taken"}` so the caller regenerates). Idempotent for the
    /// same user re-setting the same phrase. Removes the user's previous index entry.
    async fn handle_phrase_set(&self, user_id: &str, phrase: &str) -> Result<Response> {
        let norm = normalize_phrase(phrase);
        // Require at least 3 words — a real backup phrase, not a stray token.
        if norm.split(' ').filter(|w| !w.is_empty()).count() < 3 {
            return Response::from_json(&serde_json::json!({ "status": "too_short" }));
        }
        let index_key = format!("{STORAGE_KEY_PHRASE_PREFIX}{norm}");
        // Uniqueness: reject only if the phrase is already owned by a DIFFERENT user.
        if let Some(owner) = self.state.storage().get::<String>(&index_key).await.ok().flatten() {
            if owner != user_id {
                return Response::from_json(&serde_json::json!({ "status": "taken" }));
            }
        }
        let mut meta = self
            .load_user_metadata(user_id)
            .await?
            .unwrap_or_else(|| UserMetadata {
                recovery_hash_data: None,
                created_at: now_ms(),
                recovery_phrase: None,
                identity: None,
            });
        // Drop the stale reverse-index entry for a replaced phrase.
        if let Some(old) = meta.recovery_phrase.as_deref() {
            let old_norm = normalize_phrase(old);
            if old_norm != norm {
                let old_key = format!("{STORAGE_KEY_PHRASE_PREFIX}{old_norm}");
                self.state.storage().delete(&old_key).await?;
            }
        }
        // Store plaintext on the user (re-showable) + reverse index for lookup.
        meta.recovery_phrase = Some(phrase.trim().to_string());
        self.save_user_metadata(user_id, &meta).await?;
        self.state.storage().put(&index_key, user_id.to_string()).await?;
        Response::from_json(&serde_json::json!({ "status": "ok" }))
    }

    /// Resolve a phrase to its user_id for login (worker mints the JWT). Fixed-window
    /// per-phrase brute-force limiter: too many failures within the window → 429. A
    /// successful resolve clears the counter. Unknown phrase → `{user_id:null}` (and
    /// counts against the limiter).
    async fn handle_phrase_resolve(&self, phrase: &str) -> Result<Response> {
        let norm = normalize_phrase(phrase);
        let rl_key = format!("{STORAGE_KEY_PHRASE_RL_PREFIX}{norm}");
        // Load the fixed-window counter {count, window_start}.
        let (mut count, mut window_start) = match self
            .state
            .storage()
            .get::<String>(&rl_key)
            .await
            .ok()
            .flatten()
            .and_then(|s| serde_json::from_str::<(i64, i64)>(&s).ok())
        {
            Some((c, w)) => (c, w),
            None => (0, now_ms()),
        };
        let now = now_ms();
        if now - window_start > PHRASE_RL_WINDOW_MS {
            count = 0;
            window_start = now;
        }
        if count >= PHRASE_RL_MAX {
            return Ok(Response::from_json(&serde_json::json!({ "error": "rate_limited" }))?
                .with_status(429));
        }

        let index_key = format!("{STORAGE_KEY_PHRASE_PREFIX}{norm}");
        let user_id = self.state.storage().get::<String>(&index_key).await.ok().flatten();

        match user_id {
            Some(uid) => {
                // Success clears the limiter for this phrase.
                self.state.storage().delete(&rl_key).await?;
                Response::from_json(&serde_json::json!({ "user_id": uid }))
            }
            None => {
                // Miss → count the attempt.
                let val = serde_json::to_string(&(count + 1, window_start))
                    .map_err(|e| Error::RustError(format!("serialize rl: {e}")))?;
                self.state.storage().put(&rl_key, val).await?;
                Response::from_json(&serde_json::json!({ "user_id": null }))
            }
        }
    }

    /// Return the user's current plaintext phrase (re-show in Settings). None → not set.
    async fn handle_phrase_get(&self, user_id: &str) -> Result<Response> {
        let phrase = self
            .load_user_metadata(user_id)
            .await?
            .and_then(|m| m.recovery_phrase);
        Response::from_json(&serde_json::json!({ "phrase": phrase }))
    }

    async fn handle_recovery_verify(
        &self,
        user_id: &str,
        recovery_key: &str,
    ) -> Result<Response> {
        let meta = self
            .load_user_metadata(user_id)
            .await?
            .ok_or_else(|| Error::RustError("user not found".into()))?;
        let hash_data = meta
            .recovery_hash_data
            .ok_or_else(|| Error::RustError("no recovery key configured".into()))?;
        let valid = verify_recovery_key(recovery_key, &hash_data)?;
        Response::from_json(&serde_json::json!({ "valid": valid }))
    }

    /// Resolve an external identity (provider + provider_uid) to a STABLE account, creating a
    /// credential-less user on first sight and storing the identity (for delivery + admin).
    /// Provider-agnostic. Isolated from passkey/phrase — no credential is created, so the user
    /// can later add a passkey to this SAME account. Idempotent: same identity → same user_id
    /// (username is refreshed if it changed).
    async fn handle_account_resolve(
        &self,
        provider: &str,
        provider_uid: &str,
        username: Option<&str>,
    ) -> Result<Response> {
        let index_key = format!("{STORAGE_KEY_IDENTITY_PREFIX}{provider}:{provider_uid}");
        if let Some(uid) = self.state.storage().get::<String>(&index_key).await.ok().flatten() {
            // Best-effort username refresh.
            if let Some(u) = username {
                if let Some(mut meta) = self.load_user_metadata(&uid).await? {
                    let stale = meta
                        .identity
                        .as_ref()
                        .map(|i| i.username.as_deref() != Some(u))
                        .unwrap_or(true);
                    if stale {
                        meta.identity = Some(Identity {
                            provider: provider.to_string(),
                            provider_uid: provider_uid.to_string(),
                            username: Some(u.to_string()),
                        });
                        self.save_user_metadata(&uid, &meta).await?;
                    }
                }
            }
            return Response::from_json(&serde_json::json!({ "userId": uid, "created": false }));
        }
        let user_id = uuid::Uuid::new_v4().to_string();
        let meta = UserMetadata {
            recovery_hash_data: None,
            created_at: now_ms(),
            recovery_phrase: None,
            identity: Some(Identity {
                provider: provider.to_string(),
                provider_uid: provider_uid.to_string(),
                username: username.map(|s| s.to_string()),
            }),
        };
        self.save_user_metadata(&user_id, &meta).await?;
        self.state.storage().put(&index_key, user_id.clone()).await?;
        Response::from_json(&serde_json::json!({ "userId": user_id, "created": true }))
    }

    /// The user's linked identity/channel (for code delivery + admin). None → never linked.
    async fn handle_identity(&self, user_id: &str) -> Result<Response> {
        let identity = self.load_user_metadata(user_id).await?.and_then(|m| m.identity);
        Response::from_json(&serde_json::json!({ "identity": identity }))
    }

    /// How many passkeys the user has. 0 → «paid but no credentials» (admin worklist).
    async fn handle_credentials_count(&self, user_id: &str) -> Result<Response> {
        let ids = self.load_user_cred_ids(user_id).await?;
        Response::from_json(&serde_json::json!({ "count": ids.len() }))
    }

    /// Record that a story chapter became AVAILABLE in the app for this user. Idempotent: keeps
    /// the FIRST-seen timestamp. Stored next to the persona, keyed by user_id.
    async fn handle_chapter_available(&self, user_id: &str, chapter: &str) -> Result<Response> {
        let key = format!("{STORAGE_KEY_CHAPTERS_PREFIX}{user_id}");
        let stored: Option<String> = self.state.storage().get(&key).await?;
        let mut map: std::collections::BTreeMap<String, i64> = match stored {
            Some(s) => serde_json::from_str(&s)?,
            None => std::collections::BTreeMap::new(),
        };
        if !map.contains_key(chapter) {
            map.insert(chapter.to_string(), now_ms());
            self.state.storage().put(&key, serde_json::to_string(&map)?).await?;
        }
        Response::from_json(&serde_json::json!({ "ok": true }))
    }

    /// Has the user reached the app (any chapter became available)? Non-empty map → entered.
    async fn handle_has_entered(&self, user_id: &str) -> Result<Response> {
        let key = format!("{STORAGE_KEY_CHAPTERS_PREFIX}{user_id}");
        let stored: Option<String> = self.state.storage().get(&key).await?;
        let entered = match stored {
            Some(s) => !serde_json::from_str::<std::collections::BTreeMap<String, i64>>(&s)?.is_empty(),
            None => false,
        };
        Response::from_json(&serde_json::json!({ "entered": entered }))
    }

    /// The chapters (id → first-available ms) a user has reached — inspection / admin.
    async fn handle_chapters_get(&self, user_id: &str) -> Result<Response> {
        let key = format!("{STORAGE_KEY_CHAPTERS_PREFIX}{user_id}");
        let stored: Option<String> = self.state.storage().get(&key).await?;
        let chapters: serde_json::Value = match stored {
            Some(s) => serde_json::from_str(&s)?,
            None => serde_json::json!({}),
        };
        Response::from_json(&serde_json::json!({ "chapters": chapters }))
    }

    /// Store a login code keyed by its HASH, enforcing a per-user cooldown on this
    /// (user-triggered, delivered) path. Within the cooldown → {ok:false, waitMs}. Does NOT
    /// overwrite other outstanding codes for the user.
    async fn handle_code_issue(&self, user_id: &str, code_hash: &str) -> Result<Response> {
        let cd_key = format!("{STORAGE_KEY_CODE_COOLDOWN_PREFIX}{user_id}");
        let now = now_ms();
        if let Some(last) = self.state.storage().get::<i64>(&cd_key).await.ok().flatten() {
            let wait = CODE_COOLDOWN_MS - (now - last);
            if wait > 0 {
                return Response::from_json(&serde_json::json!({ "ok": false, "waitMs": wait }));
            }
        }
        self.store_code(user_id, code_hash, now).await?;
        self.state.storage().put(&cd_key, now).await?;
        Response::from_json(&serde_json::json!({ "ok": true }))
    }

    /// Store a login code (by hash) WITHOUT cooldown — for the trusted Mini App, which mints a
    /// code per /miniapp/me to embed in the onboard link. Because codes are hash-keyed, a
    /// freshly minted code does NOT invalidate one already sitting in an earlier onboard link.
    async fn handle_code_mint(&self, user_id: &str, code_hash: &str) -> Result<Response> {
        self.store_code(user_id, code_hash, now_ms()).await?;
        Response::from_json(&serde_json::json!({ "ok": true }))
    }

    async fn store_code(&self, user_id: &str, code_hash: &str, now: i64) -> Result<()> {
        let key = format!("{STORAGE_KEY_CODEHASH_PREFIX}{code_hash}");
        let rec = serde_json::json!({ "userId": user_id, "expires": now + CODE_TTL_MS });
        self.state.storage().put(&key, rec.to_string()).await
    }

    /// Verify + CONSUME a code by its hash. Match (unexpired) → delete → {ok:true, userId}.
    /// Miss/expired → {ok:false} (+ a global fixed-window limiter to blunt brute force).
    async fn handle_code_consume(&self, code_hash: &str) -> Result<Response> {
        if let Some(resp) = self.verify_rl_over_limit().await? {
            return Ok(resp);
        }
        let key = format!("{STORAGE_KEY_CODEHASH_PREFIX}{code_hash}");
        let rec = self
            .state
            .storage()
            .get::<String>(&key)
            .await
            .ok()
            .flatten()
            .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok());
        match rec {
            Some(r) if r.get("expires").and_then(|v| v.as_i64()).unwrap_or(0) > now_ms() => {
                self.state.storage().delete(&key).await?; // single-use
                let user_id = r.get("userId").and_then(|v| v.as_str()).unwrap_or("");
                Response::from_json(&serde_json::json!({ "ok": true, "userId": user_id }))
            }
            _ => {
                self.verify_rl_bump().await?;
                Response::from_json(&serde_json::json!({ "ok": false }))
            }
        }
    }

    async fn verify_rl_over_limit(&self) -> Result<Option<Response>> {
        let (count, ws) = self
            .state
            .storage()
            .get::<String>(STORAGE_KEY_VERIFY_RL)
            .await
            .ok()
            .flatten()
            .and_then(|s| serde_json::from_str::<(i64, i64)>(&s).ok())
            .unwrap_or((0, now_ms()));
        let now = now_ms();
        let count = if now - ws > VERIFY_RL_WINDOW_MS { 0 } else { count };
        if count >= VERIFY_RL_MAX {
            return Ok(Some(
                Response::from_json(&serde_json::json!({ "ok": false, "rateLimited": true }))?,
            ));
        }
        Ok(None)
    }
    async fn verify_rl_bump(&self) -> Result<()> {
        let (mut count, mut ws) = self
            .state
            .storage()
            .get::<String>(STORAGE_KEY_VERIFY_RL)
            .await
            .ok()
            .flatten()
            .and_then(|s| serde_json::from_str::<(i64, i64)>(&s).ok())
            .unwrap_or((0, now_ms()));
        let now = now_ms();
        if now - ws > VERIFY_RL_WINDOW_MS {
            count = 0;
            ws = now;
        }
        let val = serde_json::to_string(&(count + 1, ws))
            .map_err(|e| Error::RustError(format!("serialize rl: {e}")))?;
        self.state.storage().put(STORAGE_KEY_VERIFY_RL, val).await
    }
}

impl DurableObject for AuthDO {
    fn new(state: worker::durable::State, env: Env) -> Self {
        Self { state, env }
    }

    async fn fetch(&self, mut req: Request) -> Result<Response> {
        let url = req.url()?;
        let path = url.path();

        match path {
            "/register/begin" => {
                let body: serde_json::Value = req.json().await.unwrap_or_default();
                let user_id = body.get("user_id").and_then(|v| v.as_str());
                let display_name = body.get("display_name").and_then(|v| v.as_str());
                let origin = body
                    .get("origin")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| Error::RustError("missing origin".into()))?;
                self.handle_register_begin(user_id, display_name, origin).await
            }
            "/register/finish" => {
                let body: serde_json::Value = req.json().await?;
                let credential = body
                    .get("credential")
                    .cloned()
                    .ok_or_else(|| Error::RustError("missing credential".into()))?;
                let user_id = body
                    .get("user_id")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| Error::RustError("missing user_id".into()))?;
                let origin = body
                    .get("origin")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| Error::RustError("missing origin".into()))?;
                self.handle_register_finish(credential, user_id, origin).await
            }
            "/authenticate/begin" => {
                let body: serde_json::Value = req.json().await.unwrap_or_default();
                let origin = body
                    .get("origin")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| Error::RustError("missing origin".into()))?;
                self.handle_authenticate_begin(origin).await
            }
            "/authenticate/finish" => {
                let body: serde_json::Value = req.json().await?;
                let credential = body
                    .get("credential")
                    .cloned()
                    .ok_or_else(|| Error::RustError("missing credential".into()))?;
                let origin = body
                    .get("origin")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| Error::RustError("missing origin".into()))?;
                self.handle_authenticate_finish(credential, origin).await
            }
            "/account/resolve" => {
                let body: serde_json::Value = req.json().await?;
                let provider = body
                    .get("provider")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| Error::RustError("missing provider".into()))?;
                let provider_uid = body
                    .get("provider_uid")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| Error::RustError("missing provider_uid".into()))?;
                let username = body.get("username").and_then(|v| v.as_str());
                self.handle_account_resolve(provider, provider_uid, username).await
            }
            "/identity" => {
                let body: serde_json::Value = req.json().await?;
                let user_id = body
                    .get("user_id")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| Error::RustError("missing user_id".into()))?;
                self.handle_identity(user_id).await
            }
            "/credentials/count" => {
                let body: serde_json::Value = req.json().await?;
                let user_id = body
                    .get("user_id")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| Error::RustError("missing user_id".into()))?;
                self.handle_credentials_count(user_id).await
            }
            "/chapters/available" => {
                let body: serde_json::Value = req.json().await?;
                let user_id = body
                    .get("user_id")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| Error::RustError("missing user_id".into()))?;
                let chapter = body
                    .get("chapter")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| Error::RustError("missing chapter".into()))?;
                self.handle_chapter_available(user_id, chapter).await
            }
            "/has-entered" => {
                let body: serde_json::Value = req.json().await?;
                let user_id = body
                    .get("user_id")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| Error::RustError("missing user_id".into()))?;
                self.handle_has_entered(user_id).await
            }
            "/chapters/get" => {
                let body: serde_json::Value = req.json().await?;
                let user_id = body
                    .get("user_id")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| Error::RustError("missing user_id".into()))?;
                self.handle_chapters_get(user_id).await
            }
            "/code/issue" => {
                let body: serde_json::Value = req.json().await?;
                let user_id = body
                    .get("user_id")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| Error::RustError("missing user_id".into()))?;
                let code_hash = body
                    .get("code_hash")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| Error::RustError("missing code_hash".into()))?;
                self.handle_code_issue(user_id, code_hash).await
            }
            "/code/mint" => {
                let body: serde_json::Value = req.json().await?;
                let user_id = body
                    .get("user_id")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| Error::RustError("missing user_id".into()))?;
                let code_hash = body
                    .get("code_hash")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| Error::RustError("missing code_hash".into()))?;
                self.handle_code_mint(user_id, code_hash).await
            }
            "/code/consume" => {
                let body: serde_json::Value = req.json().await?;
                let code_hash = body
                    .get("code_hash")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| Error::RustError("missing code_hash".into()))?;
                self.handle_code_consume(code_hash).await
            }
            "/pair/create" => {
                let body: serde_json::Value = req.json().await?;
                let pairing_id = body
                    .get("pairing_id")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| Error::RustError("missing pairing_id".into()))?;
                let secret_hash = body
                    .get("secret_hash")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| Error::RustError("missing secret_hash".into()))?;
                let user_id = body
                    .get("user_id")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| Error::RustError("missing user_id".into()))?;
                let created_at = body
                    .get("created_at")
                    .and_then(|v| v.as_i64())
                    .ok_or_else(|| Error::RustError("missing created_at".into()))?;
                let expires_at = body
                    .get("expires_at")
                    .and_then(|v| v.as_i64())
                    .ok_or_else(|| Error::RustError("missing expires_at".into()))?;
                self.handle_pair_create(pairing_id, secret_hash, user_id, created_at, expires_at)
                    .await
            }
            "/pair/request" => {
                let body: serde_json::Value = req.json().await?;
                let pairing_id = body
                    .get("pairing_id")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| Error::RustError("missing pairing_id".into()))?;
                let secret_hash = body
                    .get("secret_hash")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| Error::RustError("missing secret_hash".into()))?;
                let created_at = body
                    .get("created_at")
                    .and_then(|v| v.as_i64())
                    .ok_or_else(|| Error::RustError("missing created_at".into()))?;
                let expires_at = body
                    .get("expires_at")
                    .and_then(|v| v.as_i64())
                    .ok_or_else(|| Error::RustError("missing expires_at".into()))?;
                self.handle_pair_request(pairing_id, secret_hash, created_at, expires_at)
                    .await
            }
            "/pair/approve" => {
                let body: serde_json::Value = req.json().await?;
                let pairing_id = body
                    .get("pairing_id")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| Error::RustError("missing pairing_id".into()))?;
                let secret_hash = body
                    .get("secret_hash")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| Error::RustError("missing secret_hash".into()))?;
                let user_id = body
                    .get("user_id")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| Error::RustError("missing user_id".into()))?;
                self.handle_pair_approve(pairing_id, secret_hash, user_id)
                    .await
            }
            "/pair/claim" => {
                let body: serde_json::Value = req.json().await?;
                let pairing_id = body
                    .get("pairing_id")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| Error::RustError("missing pairing_id".into()))?;
                let secret_hash = body
                    .get("secret_hash")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| Error::RustError("missing secret_hash".into()))?;
                self.handle_pair_claim(pairing_id, secret_hash).await
            }
            "/pair/finish" => {
                let body: serde_json::Value = req.json().await?;
                let pairing_id = body
                    .get("pairing_id")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| Error::RustError("missing pairing_id".into()))?;
                let credential = body
                    .get("credential")
                    .cloned()
                    .ok_or_else(|| Error::RustError("missing credential".into()))?;
                self.handle_pair_finish(pairing_id, credential).await
            }
            "/pair/status" | "/pair/check" => {
                let body: serde_json::Value = req.json().await?;
                let pairing_id = body
                    .get("pairing_id")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| Error::RustError("missing pairing_id".into()))?;
                self.handle_pair_status(pairing_id).await
            }
            "/token/store" => {
                let body: serde_json::Value = req.json().await?;
                let token_id = body
                    .get("token_id")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| Error::RustError("missing token_id".into()))?;
                let user_id = body
                    .get("user_id")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| Error::RustError("missing user_id".into()))?;
                let fingerprint = body
                    .get("fingerprint")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                let created_at = body
                    .get("created_at")
                    .and_then(|v| v.as_i64())
                    .ok_or_else(|| Error::RustError("missing created_at".into()))?;
                self.handle_token_store(token_id, user_id, fingerprint, created_at)
                    .await
            }
            "/token/validate" => {
                let body: serde_json::Value = req.json().await?;
                let token_id = body
                    .get("token_id")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| Error::RustError("missing token_id".into()))?;
                self.handle_token_validate(token_id).await
            }
            "/token/list" => {
                let body: serde_json::Value = req.json().await?;
                let user_id = body
                    .get("user_id")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| Error::RustError("missing user_id".into()))?;
                self.handle_token_list(user_id).await
            }
            "/recovery/set" => {
                let body: serde_json::Value = req.json().await?;
                let user_id = body
                    .get("user_id")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| Error::RustError("missing user_id".into()))?;
                let hash_data: RecoveryHashData = serde_json::from_value(
                    body.get("hash_data")
                        .cloned()
                        .ok_or_else(|| Error::RustError("missing hash_data".into()))?,
                )
                .map_err(|e| Error::RustError(format!("parse hash_data: {e}")))?;
                self.handle_recovery_set(user_id, hash_data).await
            }
            "/recovery/verify" => {
                let body: serde_json::Value = req.json().await?;
                let user_id = body
                    .get("user_id")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| Error::RustError("missing user_id".into()))?;
                let recovery_key = body
                    .get("recovery_key")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| Error::RustError("missing recovery_key".into()))?;
                self.handle_recovery_verify(user_id, recovery_key).await
            }
            "/recovery/phrase/set" => {
                let body: serde_json::Value = req.json().await?;
                let user_id = body
                    .get("user_id")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| Error::RustError("missing user_id".into()))?;
                let phrase = body
                    .get("phrase")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| Error::RustError("missing phrase".into()))?;
                self.handle_phrase_set(user_id, phrase).await
            }
            "/recovery/phrase/resolve" => {
                let body: serde_json::Value = req.json().await?;
                let phrase = body
                    .get("phrase")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| Error::RustError("missing phrase".into()))?;
                self.handle_phrase_resolve(phrase).await
            }
            "/recovery/phrase/get" => {
                let body: serde_json::Value = req.json().await?;
                let user_id = body
                    .get("user_id")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| Error::RustError("missing user_id".into()))?;
                self.handle_phrase_get(user_id).await
            }
            _ => Response::error(format!("unknown DO path: {path}"), 404),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::select_is_admin;
    const ADMIN_DEV: &str = "https://renorma-admin-dev.pages.dev";
    const ADMIN_PROD: &str = "https://admin.renorma.app";

    #[test]
    fn admin_origin_selects_admin_dev() {
        assert!(select_is_admin("https://renorma-admin-dev.pages.dev", ADMIN_DEV));
    }
    #[test]
    fn admin_origin_selects_admin_prod() {
        assert!(select_is_admin("https://admin.renorma.app", ADMIN_PROD));
    }
    #[test]
    fn app_origin_selects_app_dev() {
        assert!(!select_is_admin("https://renorma-fit-dev.pages.dev", ADMIN_DEV));
    }
    #[test]
    fn app_origin_selects_app_prod() {
        assert!(!select_is_admin("https://fit.renorma.app", ADMIN_PROD));
    }
    #[test]
    fn empty_origin_is_not_admin() {
        assert!(!select_is_admin("", ADMIN_DEV));
    }
    #[test]
    fn unknown_origin_is_not_admin() {
        assert!(!select_is_admin("https://evil.example", ADMIN_DEV));
    }
}

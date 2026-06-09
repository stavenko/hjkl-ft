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

// Storage key prefixes for the global AuthDO
const STORAGE_KEY_CRED_PREFIX: &str = "cred:";
const STORAGE_KEY_USER_CREDS_PREFIX: &str = "user_creds:";
const STORAGE_KEY_USER_PREFIX: &str = "user:";
const STORAGE_KEY_PAIRING_PREFIX: &str = "pairing:";
const STORAGE_KEY_STATE_PREFIX: &str = "pk_state:";

type HmacSha256 = Hmac<Sha256>;

// ---- Recovery hash helpers (HMAC-SHA256 based) ----

#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct RecoveryHashData {
    pub salt_b64: String,
    pub hash_b64: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct UserMetadata {
    recovery_hash_data: Option<RecoveryHashData>,
    created_at: i64,
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
    fn passkey_config(&self) -> PasskeyConfig {
        let rp_id = self
            .env
            .var("RP_ID")
            .map(|v| v.to_string())
            .unwrap_or_else(|_| "localhost".to_string());
        let rp_origin = self
            .env
            .var("RP_ORIGIN")
            .map(|v| v.to_string())
            .unwrap_or_else(|_| format!("https://{rp_id}"));
        PasskeyConfig {
            rp_id,
            rp_name: "Food Tracker".to_string(),
            origin: rp_origin,
            state_ttl: 300,
        }
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
                    if let Ok(cred) = serde_json::from_str::<StoredPasskey>(&json_str) {
                        result.push(cred);
                    }
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
                if let Ok(cred) = serde_json::from_str::<StoredPasskey>(&json) {
                    result.push(cred);
                }
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

    async fn handle_register_begin(&self, user_id: Option<&str>) -> Result<Response> {
        let user_id = match user_id {
            Some(id) if !id.is_empty() => id.to_string(),
            _ => uuid::Uuid::new_v4().to_string(),
        };

        let config = self.passkey_config();
        let credentials = self.load_user_credentials(&user_id).await?;
        let states = self.load_states().await?;
        let store = DoPasskeyStore::new(credentials, states);

        // Ensure user metadata exists
        if self.load_user_metadata(&user_id).await?.is_none() {
            let meta = UserMetadata {
                recovery_hash_data: None,
                created_at: now_ms(),
            };
            self.save_user_metadata(&user_id, &meta).await?;
        }

        let options =
            start_registration(&store, &user_id, &user_id, &user_id, &config, now_ms())
                .await
                .map_err(|e| Error::RustError(format!("registration begin: {e}")))?;

        self.flush_store(&store).await?;

        let body = serde_json::json!({ "publicKey": options, "user_id": user_id });
        Response::from_json(&body)
    }

    async fn handle_register_finish(&self, credential: serde_json::Value, user_id: &str) -> Result<Response> {
        let config = self.passkey_config();
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

    async fn handle_authenticate_begin(&self) -> Result<Response> {
        let config = self.passkey_config();
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
    ) -> Result<Response> {
        let config = self.passkey_config();
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

        // Bind user to session (keep Pending - claim will change to Claimed)
        session.user_id = user_id.to_string();
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
        if session.status != PairingStatus::Pending {
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

        // Must have a user_id bound (from approve)
        if session.user_id.is_empty() {
            return Response::error("pairing session not yet approved", 409);
        }

        session.status = PairingStatus::Claimed;
        let updated = serde_json::to_string(&session)
            .map_err(|e| Error::RustError(format!("serialize: {e}")))?;
        self.state.storage().put(&key, updated).await?;

        // Start passkey registration for the bound user
        let user_id = session.user_id.clone();
        let config = self.passkey_config();
        let credentials = self.load_user_credentials(&user_id).await?;
        let states = self.load_states().await?;
        let store = DoPasskeyStore::new(credentials, states);

        let options =
            start_registration(&store, &user_id, &user_id, &user_id, &config, now_ms())
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

        // Complete passkey registration
        let config = self.passkey_config();
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
            });
        meta.recovery_hash_data = Some(hash_data);
        self.save_user_metadata(user_id, &meta).await?;
        Response::from_json(&serde_json::json!({ "status": "ok" }))
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
                self.handle_register_begin(user_id).await
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
                self.handle_register_finish(credential, user_id).await
            }
            "/authenticate/begin" => self.handle_authenticate_begin().await,
            "/authenticate/finish" => {
                let body: serde_json::Value = req.json().await?;
                let credential = body
                    .get("credential")
                    .cloned()
                    .ok_or_else(|| Error::RustError("missing credential".into()))?;
                self.handle_authenticate_finish(credential).await
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
            "/pair/status" => {
                let body: serde_json::Value = req.json().await?;
                let pairing_id = body
                    .get("pairing_id")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| Error::RustError("missing pairing_id".into()))?;
                self.handle_pair_status(pairing_id).await
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
            _ => Response::error(format!("unknown DO path: {path}"), 404),
        }
    }
}

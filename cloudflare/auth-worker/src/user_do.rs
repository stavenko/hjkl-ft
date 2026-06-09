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

const STORAGE_KEY_CREDENTIALS: &str = "credentials";
const STORAGE_KEY_STATES_PREFIX: &str = "pk_state:";
const STORAGE_KEY_RECOVERY_HASH: &str = "recovery_hash";
const STORAGE_KEY_USERNAME: &str = "username";
const STORAGE_KEY_USER_ID: &str = "user_id";
const STORAGE_KEY_PAIRING_PREFIX: &str = "pairing:";

type HmacSha256 = Hmac<Sha256>;

// ---- Recovery hash helpers (HMAC-SHA256 based) ----

#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct RecoveryHashData {
    pub salt_b64: String,
    pub hash_b64: String,
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
pub struct UserDO {
    state: worker::durable::State,
    env: Env,
}

impl UserDO {
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

    async fn load_credentials(&self) -> Result<Vec<StoredPasskey>> {
        let creds: Option<String> = self.state.storage().get(STORAGE_KEY_CREDENTIALS).await?;
        match creds {
            Some(json) => serde_json::from_str(&json)
                .map_err(|e| Error::RustError(format!("parse credentials: {e}"))),
            None => Ok(Vec::new()),
        }
    }

    async fn load_states(&self) -> Result<Vec<(String, PasskeyState)>> {
        let map = self
            .state
            .storage()
            .list_with_options(
                worker::durable::ListOptions::new().prefix(STORAGE_KEY_STATES_PREFIX),
            )
            .await?;
        let now = now_ms();
        let mut result = Vec::new();
        let iter = js_sys::try_iter(&map)
            .ok()
            .flatten();
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
                                .strip_prefix(STORAGE_KEY_STATES_PREFIX)
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

        // Persist credentials as JSON string
        let creds_json = serde_json::to_string(&*store.credentials.borrow())
            .map_err(|e| Error::RustError(format!("serialize credentials: {e}")))?;
        storage.put(STORAGE_KEY_CREDENTIALS, creds_json).await?;

        // Delete all existing state keys
        let existing_map = storage
            .list_with_options(
                worker::durable::ListOptions::new().prefix(STORAGE_KEY_STATES_PREFIX),
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
            let storage_key = format!("{STORAGE_KEY_STATES_PREFIX}{id}");
            let state_json = serde_json::to_string(state)
                .map_err(|e| Error::RustError(format!("serialize state: {e}")))?;
            storage.put(&storage_key, state_json).await?;
        }
        Ok(())
    }

    async fn get_or_create_user_id(&self, username: &str) -> Result<String> {
        let storage = self.state.storage();
        let existing: Option<String> = storage.get(STORAGE_KEY_USER_ID).await?;
        if let Some(uid) = existing {
            return Ok(uid);
        }
        let uid = uuid::Uuid::new_v4().to_string();
        storage.put(STORAGE_KEY_USER_ID, &uid).await?;
        storage.put(STORAGE_KEY_USERNAME, username).await?;
        Ok(uid)
    }

    async fn get_user_id(&self) -> Result<Option<String>> {
        self.state.storage().get(STORAGE_KEY_USER_ID).await
    }

    // ---- Passkey handlers ----

    async fn handle_register_begin(&self, username: &str) -> Result<Response> {
        let user_id = self.get_or_create_user_id(username).await?;
        let config = self.passkey_config();
        let credentials = self.load_credentials().await?;
        let states = self.load_states().await?;
        let store = DoPasskeyStore::new(credentials, states);

        let options =
            start_registration(&store, &user_id, username, username, &config, now_ms())
                .await
                .map_err(|e| Error::RustError(format!("registration begin: {e}")))?;

        self.flush_store(&store).await?;

        let body = serde_json::json!({ "publicKey": options, "username": username, "user_id": user_id });
        Response::from_json(&body)
    }

    async fn handle_register_finish(&self, credential: serde_json::Value) -> Result<Response> {
        let user_id = self
            .get_user_id()
            .await?
            .ok_or_else(|| Error::RustError("user not found — call register_begin first".into()))?;
        let config = self.passkey_config();
        let credentials = self.load_credentials().await?;
        let states = self.load_states().await?;
        let store = DoPasskeyStore::new(credentials, states);

        let response: passkey_server::types::RegistrationResponse =
            serde_json::from_value(credential)
                .map_err(|e| Error::RustError(format!("parse credential: {e}")))?;

        finish_registration(&store, &user_id, &config, response, now_ms())
            .await
            .map_err(|e| Error::RustError(format!("registration finish: {e}")))?;

        self.flush_store(&store).await?;

        Response::from_json(&serde_json::json!({ "ok": true }))
    }

    async fn handle_authenticate_begin(&self) -> Result<Response> {
        let config = self.passkey_config();
        let credentials = self.load_credentials().await?;
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
        let credentials = self.load_credentials().await?;
        let states = self.load_states().await?;
        let store = DoPasskeyStore::new(credentials, states);

        let response: passkey_server::types::LoginResponse =
            serde_json::from_value(credential)
                .map_err(|e| Error::RustError(format!("parse credential: {e}")))?;

        let user_id = finish_login(&store, &config, response, now_ms())
            .await
            .map_err(|e| Error::RustError(format!("login finish: {e}")))?;

        self.flush_store(&store).await?;

        Response::from_json(&serde_json::json!({ "ok": true, "user_id": user_id }))
    }

    // ---- Recovery handlers ----

    async fn handle_recovery_set(&self, hash_json: String) -> Result<Response> {
        self.state
            .storage()
            .put(STORAGE_KEY_RECOVERY_HASH, hash_json)
            .await?;
        Response::from_json(&serde_json::json!({ "status": "ok" }))
    }

    async fn handle_recovery_verify(&self, recovery_key: &str) -> Result<Response> {
        let stored_json: Option<String> = self
            .state
            .storage()
            .get(STORAGE_KEY_RECOVERY_HASH)
            .await?;
        let stored_json =
            stored_json.ok_or_else(|| Error::RustError("no recovery key configured".into()))?;
        let data: RecoveryHashData = serde_json::from_str(&stored_json)
            .map_err(|e| Error::RustError(format!("parse recovery hash: {e}")))?;
        let valid = verify_recovery_key(recovery_key, &data)?;
        Response::from_json(&serde_json::json!({ "valid": valid }))
    }

    // ---- Pairing handlers ----

    fn pairing_storage_key(pairing_id: &str) -> String {
        format!("{STORAGE_KEY_PAIRING_PREFIX}{pairing_id}")
    }

    pub(crate) async fn handle_pairing_create(
        &self,
        pairing_id: &str,
        secret_hash: &str,
        username: &str,
        user_id: &str,
        created_at: i64,
        expires_at: i64,
    ) -> Result<Response> {
        let session = PairingSession {
            pairing_id: pairing_id.to_string(),
            secret_hash: secret_hash.to_string(),
            username: username.to_string(),
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

    pub(crate) async fn handle_pairing_get(&self, pairing_id: &str) -> Result<Response> {
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
                Response::from_json(&session)
            }
            None => Response::error("pairing session not found", 404),
        }
    }

    pub(crate) async fn handle_pairing_claim(&self, pairing_id: &str) -> Result<Response> {
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
                format!("pairing session already {}", serde_json::to_string(&session.status).unwrap_or_default()),
                409,
            );
        }

        session.status = PairingStatus::Claimed;
        let updated = serde_json::to_string(&session)
            .map_err(|e| Error::RustError(format!("serialize: {e}")))?;
        self.state.storage().put(&key, updated).await?;

        Response::from_json(&session)
    }

    pub(crate) async fn handle_pairing_approve(&self, pairing_id: &str, user_id: &str, username: &str) -> Result<Response> {
        let key = Self::pairing_storage_key(pairing_id);
        let stored: Option<String> = self.state.storage().get(&key).await?;
        let json_str = stored.ok_or_else(|| Error::RustError("pairing session not found".into()))?;
        let mut session: PairingSession = serde_json::from_str(&json_str)
            .map_err(|e| Error::RustError(format!("parse pairing session: {e}")))?;

        session.status = PairingStatus::Claimed;
        session.user_id = user_id.to_string();
        session.username = username.to_string();
        let updated = serde_json::to_string(&session)
            .map_err(|e| Error::RustError(format!("serialize: {e}")))?;
        self.state.storage().put(&key, updated).await?;

        Response::from_json(&serde_json::json!({ "ok": true }))
    }

    pub(crate) async fn handle_pairing_complete(&self, pairing_id: &str) -> Result<Response> {
        let key = Self::pairing_storage_key(pairing_id);
        let stored: Option<String> = self.state.storage().get(&key).await?;
        let json_str =
            stored.ok_or_else(|| Error::RustError("pairing session not found".into()))?;
        let mut session: PairingSession = serde_json::from_str(&json_str)
            .map_err(|e| Error::RustError(format!("parse pairing session: {e}")))?;

        if session.status != PairingStatus::Claimed {
            return Response::error("pairing session not in claimed state", 409);
        }

        session.status = PairingStatus::Completed;
        let updated = serde_json::to_string(&session)
            .map_err(|e| Error::RustError(format!("serialize: {e}")))?;
        self.state.storage().put(&key, updated).await?;

        Response::from_json(&serde_json::json!({ "ok": true }))
    }
}

impl DurableObject for UserDO {
    fn new(state: worker::durable::State, env: Env) -> Self {
        Self { state, env }
    }

    async fn fetch(&self, mut req: Request) -> Result<Response> {
        let url = req.url()?;
        let path = url.path();

        match path {
            "/passkey/register/begin" => {
                let body: serde_json::Value = req.json().await?;
                let username = body
                    .get("username")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| Error::RustError("missing username".into()))?;
                self.handle_register_begin(username).await
            }
            "/passkey/register/finish" => {
                let body: serde_json::Value = req.json().await?;
                let credential = body
                    .get("credential")
                    .cloned()
                    .ok_or_else(|| Error::RustError("missing credential".into()))?;
                self.handle_register_finish(credential).await
            }
            "/passkey/authenticate/begin" => self.handle_authenticate_begin().await,
            "/passkey/authenticate/finish" => {
                let body: serde_json::Value = req.json().await?;
                let credential = body
                    .get("credential")
                    .cloned()
                    .ok_or_else(|| Error::RustError("missing credential".into()))?;
                self.handle_authenticate_finish(credential).await
            }
            "/recovery/set" => {
                let hash_json: String = req.text().await?;
                self.handle_recovery_set(hash_json).await
            }
            "/recovery/verify" => {
                let body: serde_json::Value = req.json().await?;
                let recovery_key = body
                    .get("recovery_key")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| Error::RustError("missing recovery_key".into()))?;
                self.handle_recovery_verify(recovery_key).await
            }
            "/pairing/create" => {
                let body: serde_json::Value = req.json().await?;
                let pairing_id = body.get("pairing_id").and_then(|v| v.as_str())
                    .ok_or_else(|| Error::RustError("missing pairing_id".into()))?;
                let secret_hash = body.get("secret_hash").and_then(|v| v.as_str())
                    .ok_or_else(|| Error::RustError("missing secret_hash".into()))?;
                let username = body.get("username").and_then(|v| v.as_str())
                    .ok_or_else(|| Error::RustError("missing username".into()))?;
                let user_id = body.get("user_id").and_then(|v| v.as_str())
                    .ok_or_else(|| Error::RustError("missing user_id".into()))?;
                let created_at = body.get("created_at").and_then(|v| v.as_i64())
                    .ok_or_else(|| Error::RustError("missing created_at".into()))?;
                let expires_at = body.get("expires_at").and_then(|v| v.as_i64())
                    .ok_or_else(|| Error::RustError("missing expires_at".into()))?;
                self.handle_pairing_create(pairing_id, secret_hash, username, user_id, created_at, expires_at).await
            }
            "/pairing/get" => {
                let body: serde_json::Value = req.json().await?;
                let pairing_id = body.get("pairing_id").and_then(|v| v.as_str())
                    .ok_or_else(|| Error::RustError("missing pairing_id".into()))?;
                self.handle_pairing_get(pairing_id).await
            }
            "/pairing/claim" => {
                let body: serde_json::Value = req.json().await?;
                let pairing_id = body.get("pairing_id").and_then(|v| v.as_str())
                    .ok_or_else(|| Error::RustError("missing pairing_id".into()))?;
                self.handle_pairing_claim(pairing_id).await
            }
            "/pairing/approve" => {
                let body: serde_json::Value = req.json().await?;
                let pairing_id = body.get("pairing_id").and_then(|v| v.as_str())
                    .ok_or_else(|| Error::RustError("missing pairing_id".into()))?;
                let user_id = body.get("user_id").and_then(|v| v.as_str())
                    .ok_or_else(|| Error::RustError("missing user_id".into()))?;
                let username = body.get("username").and_then(|v| v.as_str())
                    .ok_or_else(|| Error::RustError("missing username".into()))?;
                self.handle_pairing_approve(pairing_id, user_id, username).await
            }
            "/pairing/complete" => {
                let body: serde_json::Value = req.json().await?;
                let pairing_id = body.get("pairing_id").and_then(|v| v.as_str())
                    .ok_or_else(|| Error::RustError("missing pairing_id".into()))?;
                self.handle_pairing_complete(pairing_id).await
            }
            _ => Response::error(format!("unknown DO path: {path}"), 404),
        }
    }
}

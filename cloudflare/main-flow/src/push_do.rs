use worker::*;
use worker::durable::State;
use wasm_bindgen::JsCast;

use crate::PushSubscription;

const SUB_PREFIX: &str = "push_sub:";
const USER_SUBS_PREFIX: &str = "user_push_subs:";

#[durable_object]
pub struct PushDO {
    state: State,
    env: Env,
}

impl DurableObject for PushDO {
    fn new(state: State, env: Env) -> Self {
        Self { state, env }
    }

    async fn fetch(&self, mut req: Request) -> Result<Response> {
        let url = req.url()?;
        let path = url.path();

        match path {
            "/subscribe" => {
                let body: serde_json::Value = req.json().await?;
                let user_id = body.get("user_id").and_then(|v| v.as_str())
                    .ok_or_else(|| Error::RustError("missing user_id".into()))?;
                let subscription: PushSubscription = serde_json::from_value(
                    body.get("subscription").cloned()
                        .ok_or_else(|| Error::RustError("missing subscription".into()))?,
                ).map_err(|e| Error::RustError(format!("parse subscription: {e}")))?;
                self.handle_subscribe(user_id, subscription).await
            }
            "/unsubscribe" => {
                let body: serde_json::Value = req.json().await?;
                let user_id = body.get("user_id").and_then(|v| v.as_str())
                    .ok_or_else(|| Error::RustError("missing user_id".into()))?;
                let endpoint = body.get("endpoint").and_then(|v| v.as_str())
                    .ok_or_else(|| Error::RustError("missing endpoint".into()))?;
                self.handle_unsubscribe(user_id, endpoint).await
            }
            "/unsubscribe-by-endpoint" => {
                let body: serde_json::Value = req.json().await?;
                let endpoint = body.get("endpoint").and_then(|v| v.as_str())
                    .ok_or_else(|| Error::RustError("missing endpoint".into()))?;
                self.handle_unsubscribe_by_endpoint(endpoint).await
            }
            "/list" => {
                let body: serde_json::Value = req.json().await?;
                let user_id = body.get("user_id").and_then(|v| v.as_str())
                    .ok_or_else(|| Error::RustError("missing user_id".into()))?;
                self.handle_list_user(user_id).await
            }
            "/list-all" => {
                self.handle_list_all().await
            }
            _ => Response::error(format!("unknown path: {path}"), 404),
        }
    }
}

impl PushDO {
    fn endpoint_hash(endpoint: &str) -> String {
        use sha2::Digest;
        use base64::Engine;
        let hash = sha2::Sha256::digest(endpoint.as_bytes());
        base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(&hash[..16])
    }

    async fn handle_subscribe(&self, user_id: &str, subscription: PushSubscription) -> Result<Response> {
        let hash = Self::endpoint_hash(&subscription.endpoint);
        let sub_key = format!("{SUB_PREFIX}{user_id}:{hash}");
        let sub_json = serde_json::to_string(&subscription)
            .map_err(|e| Error::RustError(format!("serialize: {e}")))?;
        self.state.storage().put(&sub_key, sub_json).await?;

        let list_key = format!("{USER_SUBS_PREFIX}{user_id}");
        let stored: Option<String> = self.state.storage().get(&list_key).await?;
        let mut hashes: Vec<String> = match stored {
            Some(json) => serde_json::from_str(&json)
                .map_err(|e| Error::RustError(format!("parse: {e}")))?,
            None => Vec::new(),
        };
        if !hashes.contains(&hash) {
            hashes.push(hash);
            let list_json = serde_json::to_string(&hashes)
                .map_err(|e| Error::RustError(format!("serialize: {e}")))?;
            self.state.storage().put(&list_key, list_json).await?;
        }

        Response::from_json(&serde_json::json!({ "ok": true }))
    }

    async fn handle_unsubscribe(&self, user_id: &str, endpoint: &str) -> Result<Response> {
        let hash = Self::endpoint_hash(endpoint);
        let sub_key = format!("{SUB_PREFIX}{user_id}:{hash}");
        self.state.storage().delete(&sub_key).await?;

        let list_key = format!("{USER_SUBS_PREFIX}{user_id}");
        let stored: Option<String> = self.state.storage().get(&list_key).await?;
        if let Some(json) = stored {
            let mut hashes: Vec<String> = serde_json::from_str(&json)
                .map_err(|e| Error::RustError(format!("parse: {e}")))?;
            hashes.retain(|h| h != &hash);
            let list_json = serde_json::to_string(&hashes)
                .map_err(|e| Error::RustError(format!("serialize: {e}")))?;
            self.state.storage().put(&list_key, list_json).await?;
        }

        Response::from_json(&serde_json::json!({ "ok": true }))
    }

    async fn handle_unsubscribe_by_endpoint(&self, endpoint: &str) -> Result<Response> {
        let hash = Self::endpoint_hash(endpoint);
        let map = self.state.storage()
            .list_with_options(
                worker::durable::ListOptions::new().prefix(SUB_PREFIX),
            )
            .await?;

        let iter = js_sys::try_iter(&map).ok().flatten();
        if let Some(iter) = iter {
            for entry in iter {
                let entry = entry.map_err(Error::from)?;
                let arr: js_sys::Array = entry.unchecked_into();
                let key_val = arr.get(0);
                if let Some(key) = key_val.as_string() {
                    if key.ends_with(&format!(":{hash}")) {
                        self.state.storage().delete(&key).await?;
                    }
                }
            }
        }

        Response::from_json(&serde_json::json!({ "ok": true }))
    }

    async fn handle_list_user(&self, user_id: &str) -> Result<Response> {
        let prefix = format!("{SUB_PREFIX}{user_id}:");
        let map = self.state.storage()
            .list_with_options(
                worker::durable::ListOptions::new().prefix(&prefix),
            )
            .await?;

        let mut subscriptions = Vec::new();
        let iter = js_sys::try_iter(&map).ok().flatten();
        if let Some(iter) = iter {
            for entry in iter {
                let entry = entry.map_err(Error::from)?;
                let arr: js_sys::Array = entry.unchecked_into();
                let val = arr.get(1);
                if let Ok(json_str) = serde_wasm_bindgen::from_value::<String>(val) {
                    if let Ok(sub) = serde_json::from_str::<PushSubscription>(&json_str) {
                        subscriptions.push(sub);
                    }
                }
            }
        }

        Response::from_json(&subscriptions)
    }

    async fn handle_list_all(&self) -> Result<Response> {
        let map = self.state.storage()
            .list_with_options(
                worker::durable::ListOptions::new().prefix(SUB_PREFIX),
            )
            .await?;

        let mut subscriptions = Vec::new();
        let iter = js_sys::try_iter(&map).ok().flatten();
        if let Some(iter) = iter {
            for entry in iter {
                let entry = entry.map_err(Error::from)?;
                let arr: js_sys::Array = entry.unchecked_into();
                let val = arr.get(1);
                if let Ok(json_str) = serde_wasm_bindgen::from_value::<String>(val) {
                    if let Ok(sub) = serde_json::from_str::<PushSubscription>(&json_str) {
                        subscriptions.push(sub);
                    }
                }
            }
        }

        Response::from_json(&subscriptions)
    }
}

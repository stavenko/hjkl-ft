use worker::*;

/// Single global index (idFromName("index")): maps "order:<id>" / "contract:<id>"
/// / "claim-contract:<id>" → userId (or guest claimId). KV-backed.
#[durable_object]
pub struct PaymentIndexDO {
    state: worker::durable::State,
    #[allow(dead_code)]
    env: Env,
}

impl DurableObject for PaymentIndexDO {
    fn new(state: worker::durable::State, env: Env) -> Self {
        Self { state, env }
    }

    async fn fetch(&self, mut req: Request) -> Result<Response> {
        let url = req.url()?;
        let path = url.path().to_string();
        let method = req.method();

        match (method, path.as_str()) {
            (Method::Post, "/put") => {
                let b: serde_json::Value = req.json().await?;
                let key = b
                    .get("key")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| Error::RustError("missing key".into()))?;
                let user_id = b
                    .get("userId")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| Error::RustError("missing userId".into()))?;
                self.state.storage().put(key, user_id.to_string()).await?;
                Response::from_json(&serde_json::json!({ "ok": true }))
            }
            (Method::Post, "/delete") => {
                let b: serde_json::Value = req.json().await?;
                let key = b
                    .get("key")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| Error::RustError("missing key".into()))?;
                self.state.storage().delete(key).await?;
                Response::from_json(&serde_json::json!({ "ok": true }))
            }
            // Drop every index entry that points at this account — otherwise a later
            // provider webhook would still route a payment to the erased user.
            (Method::Post, "/forget-user") => {
                let b: serde_json::Value = req.json().await?;
                let user_id = b
                    .get("userId")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| Error::RustError("missing userId".into()))?
                    .to_string();
                let storage = self.state.storage();
                let listed = storage
                    .list_with_options(worker::durable::ListOptions::new().prefix("contract:"))
                    .await?;
                let mut victims: Vec<String> = Vec::new();
                for entry in listed.entries() {
                    let entry =
                        entry.map_err(|e| Error::RustError(format!("index entry: {e:?}")))?;
                    let pair: Vec<serde_json::Value> = serde_wasm_bindgen::from_value(entry)
                        .map_err(|e| Error::RustError(format!("index pair: {e}")))?;
                    let key = pair
                        .first()
                        .and_then(|k| k.as_str())
                        .ok_or_else(|| Error::RustError("index entry without key".into()))?;
                    if pair.get(1).and_then(|v| v.as_str()) == Some(user_id.as_str()) {
                        victims.push(key.to_string());
                    }
                }
                let mut deleted = 0u32;
                for key in &victims {
                    if storage.delete(key).await? {
                        deleted += 1;
                    }
                }
                Response::from_json(&serde_json::json!({ "ok": true, "deleted": deleted }))
            }
            (Method::Get, "/get") => {
                let key = url
                    .query_pairs()
                    .find(|(k, _)| k == "key")
                    .map(|(_, v)| v.to_string())
                    .unwrap_or_default();
                let user_id: Option<String> = self.state.storage().get::<String>(&key).await?;
                Response::from_json(&serde_json::json!({ "userId": user_id }))
            }
            _ => Response::error("Not found", 404),
        }
    }
}

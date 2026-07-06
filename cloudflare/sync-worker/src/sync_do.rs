use std::collections::BTreeMap;
use worker::*;

/// Collections keyed by `id`, last-writer-wins by `updated_at`. Mirrors the TS
/// `ID_COLLECTIONS`. `deletions` is append-only by id (tombstone accumulator).
const ID_COLLECTIONS: [&str; 8] = [
    "foods",
    "diary_entries",
    "recipes",
    "recipe_ingredients",
    "goals",
    "weight_entries",
    "step_entries",
    "deletions",
];

/// `true` when `incoming` should overwrite `current` (newer, or current absent).
/// Compares the RFC3339 `updated_at` strings lexicographically — identical to the
/// TS `isNewer`.
fn is_newer(incoming: &serde_json::Value, current: Option<&serde_json::Value>) -> bool {
    let cur = match current {
        None => return true,
        Some(c) => c,
    };
    let inc_ts = incoming.get("updated_at").and_then(|v| v.as_str()).unwrap_or("");
    let cur_ts = cur.get("updated_at").and_then(|v| v.as_str()).unwrap_or("");
    inc_ts > cur_ts
}

/// Per-user data store: one instance per JWT `sub` (idFromName(sub)). Records merge
/// last-writer-wins by their RFC3339 `updated_at`. Each collection is a JSON map
/// (id/key → row) stored under its collection name — identical to the TS SyncDO.
#[durable_object]
pub struct SyncDO {
    state: worker::durable::State,
    #[allow(dead_code)]
    env: Env,
}

impl SyncDO {
    /// Load a collection map (id/key → row); absent → empty map. Mirrors TS `map`.
    async fn map(&self, name: &str) -> Result<BTreeMap<String, serde_json::Value>> {
        Ok(self
            .state
            .storage()
            .get::<BTreeMap<String, serde_json::Value>>(name)
            .await?
            .unwrap_or_default())
    }

    async fn put_map(&self, name: &str, m: &BTreeMap<String, serde_json::Value>) -> Result<()> {
        self.state.storage().put(name, m).await
    }

    async fn dump(&self) -> Result<serde_json::Value> {
        let mut out = serde_json::Map::new();
        for name in ID_COLLECTIONS {
            let m = self.map(name).await?;
            let values: Vec<serde_json::Value> = if name == "diary_entries" {
                m.into_values()
                    .filter(|r| !r.get("deleted").and_then(|v| v.as_bool()).unwrap_or(false))
                    .collect()
            } else {
                m.into_values().collect()
            };
            out.insert(name.to_string(), serde_json::Value::Array(values));
        }
        let story = self.map("story").await?;
        out.insert(
            "story".to_string(),
            serde_json::Value::Array(story.into_values().collect()),
        );
        let profile = self.map("profile").await?;
        out.insert(
            "profile".to_string(),
            serde_json::Value::Array(profile.into_values().collect()),
        );
        Ok(serde_json::Value::Object(out))
    }

    async fn push(&self, payload: &serde_json::Value) -> Result<()> {
        for name in ID_COLLECTIONS {
            let incoming = match payload.get(name).and_then(|v| v.as_array()) {
                Some(arr) if !arr.is_empty() => arr,
                _ => continue,
            };
            let mut m = self.map(name).await?;
            for row in incoming {
                let id = match row.get("id").and_then(|v| v.as_str()) {
                    Some(id) => id.to_string(),
                    None => continue,
                };
                let cur = m.get(&id);
                let deleted = row.get("deleted").and_then(|v| v.as_bool()).unwrap_or(false);
                if name == "diary_entries" && deleted {
                    // Tombstone: keep it (so dump omits the row) when it's newer.
                    if is_newer(row, cur) {
                        m.insert(id, row.clone());
                    }
                } else if is_newer(row, cur) {
                    m.insert(id, row.clone());
                }
            }
            self.put_map(name, &m).await?;
        }

        if let Some(arr) = payload.get("story").and_then(|v| v.as_array()) {
            if !arr.is_empty() {
                let mut m = self.map("story").await?;
                for row in arr {
                    let key = match row.get("key").and_then(|v| v.as_str()) {
                        Some(k) => k.to_string(),
                        None => continue,
                    };
                    if is_newer(row, m.get(&key)) {
                        m.insert(key, row.clone());
                    }
                }
                self.put_map("story", &m).await?;
            }
        }

        // Profile is a keyed singleton (one key, "profile") — same LWW machinery as
        // story: whole-object last-writer-wins, no per-field merge.
        if let Some(arr) = payload.get("profile").and_then(|v| v.as_array()) {
            if !arr.is_empty() {
                let mut m = self.map("profile").await?;
                for row in arr {
                    let key = match row.get("key").and_then(|v| v.as_str()) {
                        Some(k) => k.to_string(),
                        None => continue,
                    };
                    if is_newer(row, m.get(&key)) {
                        m.insert(key, row.clone());
                    }
                }
                self.put_map("profile", &m).await?;
            }
        }

        Ok(())
    }
}

impl DurableObject for SyncDO {
    fn new(state: worker::durable::State, env: Env) -> Self {
        Self { state, env }
    }

    async fn fetch(&self, mut req: Request) -> Result<Response> {
        let url = req.url()?;
        let path = url.path().to_string();
        let method = req.method();

        match (method, path.as_str()) {
            (Method::Post, "/sync/dump") => Response::from_json(&self.dump().await?),
            (Method::Post, "/sync/push") => {
                let payload: serde_json::Value = req.json().await?;
                self.push(&payload).await?;
                Response::from_json(&serde_json::json!({ "conflicts": null }))
            }
            _ => Response::error("Not found", 404),
        }
    }
}

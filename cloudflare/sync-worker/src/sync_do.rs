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

// ── Sync v2: journaled incremental batches ───────────────────────────────────
// The client forms the change list (its outbox); a push appends ONE batch to a
// journal under `version = last + 1` and applies it to the same materialized
// maps v1 uses. A pull returns only the journal tail (`version > since`),
// applied by the client strictly in order. Versions are integers; no wall
// clocks involved.
//
// The SOURCE OF TRUTH is the client's local database — this DO is dumb storage
// and never synthesizes data. A store starts UNINITIALIZED: pulls are refused
// (409 store_not_initialized) until the first client MIGRATES its local data in
// (one day-batch at a time) and confirms the batch count via /sync/v2/init,
// which flips the initialized flag. From then on the journal is the complete
// data source for every device.

const V2_LAST: &str = "v2:last_version";
const V2_OLDEST: &str = "v2:oldest_version";
const V2_INIT: &str = "v2:initialized";

/// (client store name, server collection name) — every v2-synced store.
const CLIENT_STORES: &[(&str, &str)] = &[
    ("foods", "foods"),
    ("diary", "diary_entries"),
    ("recipes", "recipes"),
    ("recipe_ingredients", "recipe_ingredients"),
    ("goals", "goals"),
    ("weight_entries", "weight_entries"),
    ("step_entries", "step_entries"),
    ("profile", "profile"),
    ("app_flags", "app_flags"),
    ("ind_days", "ind_days"),
];

/// Server collection for a client store name (also accepts deletion-record
/// `kind` values, which use the client store names).
fn collection_for(store: &str) -> Option<&'static str> {
    CLIENT_STORES.iter().find(|(c, _)| *c == store).map(|(_, s)| *s)
}

/// The row key field of a client store.
fn key_field_for(store: &str) -> &'static str {
    match store {
        "profile" | "app_flags" => "key",
        _ => "id",
    }
}

// v2 applies batch changes UNCONDITIONALLY, in journal order: a batch is only
// accepted from a client whose base_version == last (compare-and-swap), i.e.
// the pusher has already SEEN and MERGED every earlier batch — its changes ARE
// the merge outcome. `updated_at` is informational; conflict resolution happens
// client-side during the merge (by event time; ind_days keeps first-wins).

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
        let app_flags = self.map("app_flags").await?;
        out.insert(
            "app_flags".to_string(),
            serde_json::Value::Array(app_flags.into_values().collect()),
        );
        let ind_days = self.map("ind_days").await?;
        out.insert(
            "ind_days".to_string(),
            serde_json::Value::Array(ind_days.into_values().collect()),
        );
        Ok(serde_json::Value::Object(out))
    }

    async fn push(&self, payload: &serde_json::Value) -> Result<()> {
        // v1→v2 bridge: every row/deletion a v1 client gets ACCEPTED here is also
        // journaled as a v2 batch, so v2 clients receive old-client changes during
        // the transition. Re-applying the batch to the maps in v2_append_batch is
        // idempotent (equal rows lose LWW/FWW; repeated deletes are no-ops).
        let mut bridge: Vec<serde_json::Value> = Vec::new();
        for name in ID_COLLECTIONS {
            let incoming = match payload.get(name).and_then(|v| v.as_array()) {
                Some(arr) if !arr.is_empty() => arr,
                _ => continue,
            };
            let client_store = if name == "diary_entries" { "diary" } else { name };
            let mut m = self.map(name).await?;
            for row in incoming {
                let id = match row.get("id").and_then(|v| v.as_str()) {
                    Some(id) => id.to_string(),
                    None => continue,
                };
                let cur = m.get(&id);
                let accepted = is_newer(row, cur);
                if accepted {
                    m.insert(id, row.clone());
                    if name == "deletions" {
                        // Translate a v1 deletion record into a v2 delete op (the
                        // record itself also stays in the log for v1 peers).
                        let kind = row.get("kind").and_then(|v| v.as_str()).unwrap_or("");
                        let target = row.get("target_id").and_then(|v| v.as_str()).unwrap_or("");
                        if collection_for(kind).is_some() && !target.is_empty() {
                            bridge.push(serde_json::json!({
                                "store": kind, "op": "delete", "id": target,
                            }));
                        } else if !kind.is_empty() {
                            console_error!("sync v1 bridge: deletion with unknown kind {kind:?}");
                        }
                    } else {
                        bridge.push(serde_json::json!({
                            "store": client_store, "op": "upsert", "row": row,
                        }));
                    }
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
                        bridge.push(serde_json::json!({ "store": "profile", "op": "upsert", "row": row }));
                    }
                }
                self.put_map("profile", &m).await?;
            }
        }

        // Cross-device app flags (story progress, indicator unlocks, letters …):
        // keyed by `key`, whole-row LWW by `updated_at`. Device-local keys never
        // reach us — the client filters them before pushing.
        if let Some(arr) = payload.get("app_flags").and_then(|v| v.as_array()) {
            if !arr.is_empty() {
                let mut m = self.map("app_flags").await?;
                for row in arr {
                    let key = match row.get("key").and_then(|v| v.as_str()) {
                        Some(k) => k.to_string(),
                        None => continue,
                    };
                    if is_newer(row, m.get(&key)) {
                        m.insert(key, row.clone());
                        bridge.push(serde_json::json!({ "store": "app_flags", "op": "upsert", "row": row }));
                    }
                }
                self.put_map("app_flags", &m).await?;
            }
        }

        // Frozen indicator-day values, keyed `"<indicator>:<date>"`. Each is
        // computed ONCE by the first device that had the data — FIRST-writer-wins
        // by `computed_at`: a row is accepted only when the key is absent or the
        // incoming computation is strictly EARLIER than the stored one (an empty
        // stamp sorts earliest, so pre-stamp legacy values keep priority).
        if let Some(arr) = payload.get("ind_days").and_then(|v| v.as_array()) {
            if !arr.is_empty() {
                let mut m = self.map("ind_days").await?;
                for row in arr {
                    let key = match row.get("id").and_then(|v| v.as_str()) {
                        Some(k) => k.to_string(),
                        None => continue,
                    };
                    let accept = match m.get(&key) {
                        None => true,
                        Some(cur) => {
                            let inc = row.get("computed_at").and_then(|v| v.as_str()).unwrap_or("");
                            let cur_ts =
                                cur.get("computed_at").and_then(|v| v.as_str()).unwrap_or("");
                            inc < cur_ts
                        }
                    };
                    if accept {
                        m.insert(key, row.clone());
                        bridge.push(serde_json::json!({ "store": "ind_days", "op": "upsert", "row": row }));
                    }
                }
                self.put_map("ind_days", &m).await?;
            }
        }

        // Journal everything this v1 push actually changed (no-op when nothing did).
        self.v2_append_batch(0, bridge).await?;

        Ok(())
    }

    // ── v2 handlers ──────────────────────────────────────────────────────────

    /// Apply `changes` to the materialized maps (per-store acceptance rules) and
    /// append them to the journal as ONE batch. Returns the resulting version
    /// (unchanged when `changes` is empty). Shared by /sync/v2/push and the v1
    /// bridge — double application is idempotent by construction.
    async fn v2_append_batch(
        &self,
        base_version: u64,
        changes: Vec<serde_json::Value>,
    ) -> Result<u64> {
        let storage = self.state.storage();
        let last: u64 = storage.get(V2_LAST).await?.unwrap_or(0);
        if changes.is_empty() {
            return Ok(last);
        }

        let mut maps: std::collections::HashMap<&'static str, BTreeMap<String, serde_json::Value>> =
            std::collections::HashMap::new();
        for ch in &changes {
            let Some(store) = ch.get("store").and_then(|v| v.as_str()) else {
                console_error!("sync v2: change without store: {ch}");
                continue;
            };
            let Some(coll) = collection_for(store) else {
                console_error!("sync v2: unknown store {store:?}");
                continue;
            };
            if !maps.contains_key(coll) {
                let m = self.map(coll).await?;
                maps.insert(coll, m);
            }
            let m = maps.get_mut(coll).expect("just inserted");
            match ch.get("op").and_then(|v| v.as_str()) {
                Some("upsert") => {
                    let Some(row) = ch.get("row") else {
                        console_error!("sync v2: upsert without row: {ch}");
                        continue;
                    };
                    let kf = key_field_for(store);
                    let Some(key) = row.get(kf).and_then(|v| v.as_str()).map(str::to_string) else {
                        console_error!("sync v2: upsert row without key field {kf}: {ch}");
                        continue;
                    };
                    m.insert(key, row.clone());
                }
                Some("delete") => {
                    if let Some(id) = ch.get("id").and_then(|v| v.as_str()) {
                        m.remove(id);
                    } else {
                        console_error!("sync v2: delete without id: {ch}");
                    }
                }
                other => console_error!("sync v2: unknown op {other:?}"),
            }
        }
        for (coll, m) in &maps {
            self.put_map(coll, m).await?;
        }

        let version = last + 1;
        storage
            .put(
                &format!("j:{version:020}"),
                &serde_json::json!({
                    "version": version,
                    "base_version": base_version,
                    "changes": changes,
                }),
            )
            .await?;
        storage.put(V2_LAST, version).await?;
        if storage.get::<u64>(V2_OLDEST).await?.is_none() {
            storage.put(V2_OLDEST, version).await?;
        }
        Ok(version)
    }

    async fn v2_push(&self, body: &serde_json::Value) -> Result<Response> {
        let base_version = body.get("base_version").and_then(|v| v.as_u64()).unwrap_or(0);
        // Compare-and-swap on the VERSION: a client may only push what it built
        // on top of the latest state. A stale base ⇒ reject; the client pulls
        // the newer batches, merges, and retries. The DO is single-threaded, so
        // this check + append are atomic — no version races.
        let last: u64 = self.state.storage().get(V2_LAST).await?.unwrap_or(0);
        if base_version != last {
            return Response::from_json(
                &serde_json::json!({ "version": last, "accepted": false }),
            );
        }
        let changes = body
            .get("changes")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();
        let version = self.v2_append_batch(base_version, changes).await?;
        Response::from_json(&serde_json::json!({ "version": version, "accepted": true }))
    }

    /// Journal tail newer than the client's version, strictly ordered. The
    /// JOURNAL IS THE ONLY DATA SOURCE for v2 clients: a zero client (since 0)
    /// simply replays it from the beginning. NOTHING can be read from an
    /// uninitialized store — the first client must migrate its local data in
    /// first (/sync/v2/push day-batches + /sync/v2/init).
    async fn v2_pull(&self, body: &serde_json::Value) -> Result<Response> {
        let storage = self.state.storage();
        if !storage.get::<bool>(V2_INIT).await?.unwrap_or(false) {
            let last: u64 = storage.get(V2_LAST).await?.unwrap_or(0);
            return Ok(Response::from_json(
                &serde_json::json!({ "error": "store_not_initialized", "version": last }),
            )?
            .with_status(409));
        }
        let since = body.get("since_version").and_then(|v| v.as_u64()).unwrap_or(0);
        let last: u64 = storage.get(V2_LAST).await?.unwrap_or(0);

        if since >= last {
            return Response::from_json(&serde_json::json!({ "version": last, "batches": [] }));
        }
        let mut batches = Vec::new();
        for v in (since + 1)..=last {
            match storage
                .get::<serde_json::Value>(&format!("j:{v:020}"))
                .await?
            {
                Some(b) => batches.push(b),
                // The journal is complete by construction (genesis) — a hole is
                // corrupted state, and we fail loudly rather than serve gaps.
                None => {
                    return Err(Error::RustError(format!(
                        "sync v2: journal hole at version {v} (last {last})"
                    )))
                }
            }
        }
        Response::from_json(&serde_json::json!({ "version": last, "batches": batches }))
    }

    /// Finalize the client-driven migration: the client uploaded its local data
    /// as sequential day-batches and now confirms the resulting version. The
    /// counts must MATCH exactly (each batch bumps the version by 1, so
    /// `last_version == expected_version` ⇔ the server holds exactly the batches
    /// the client sent) — only then does the store become initialized and
    /// readable. A mismatch is a loud 409, never a silent acceptance.
    async fn v2_init(&self, body: &serde_json::Value) -> Result<Response> {
        let storage = self.state.storage();
        let expected = body
            .get("expected_version")
            .and_then(|v| v.as_u64())
            .ok_or_else(|| Error::RustError("sync v2: init without expected_version".into()))?;
        let last: u64 = storage.get(V2_LAST).await?.unwrap_or(0);
        if last != expected {
            return Ok(Response::from_json(&serde_json::json!({
                "error": "init_version_mismatch", "version": last,
            }))?
            .with_status(409));
        }
        storage.put(V2_INIT, true).await?;
        Response::from_json(&serde_json::json!({ "version": last, "accepted": true }))
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
            (Method::Post, "/sync/v2/push") => {
                let payload: serde_json::Value = req.json().await?;
                self.v2_push(&payload).await
            }
            (Method::Post, "/sync/v2/pull") => {
                let payload: serde_json::Value = req.json().await?;
                self.v2_pull(&payload).await
            }
            (Method::Post, "/sync/v2/init") => {
                let payload: serde_json::Value = req.json().await?;
                self.v2_init(&payload).await
            }
            _ => Response::error("Not found", 404),
        }
    }
}

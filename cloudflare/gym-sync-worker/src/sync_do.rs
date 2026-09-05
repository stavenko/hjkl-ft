use worker::*;

// ── Sync v2: a journaled batch log, nothing else ─────────────────────────────
// The SOURCE OF TRUTH is the client's local database — this DO is dumb storage
// and never inspects or synthesizes data. The client forms the change list
// (its outbox); a push appends ONE batch to the journal under
// `version = last + 1` (compare-and-swap on the version). A pull returns only
// the journal tail (`version > since`), applied by the client strictly in
// order. Versions are integers; no wall clocks involved.
//
// A store starts UNINITIALIZED: pulls are refused (409 store_not_initialized)
// until the first client MIGRATES its local data in (one day-batch at a time)
// and confirms the batch count via /sync/v2/init, which flips the initialized
// flag. From then on the journal is the complete data source for every device.

const V2_LAST: &str = "v2:last_version";
const V2_INIT: &str = "v2:initialized";

/// Per-user journal store: one instance per JWT `sub` (idFromName(sub)).
#[durable_object]
pub struct SyncDO {
    state: worker::durable::State,
    #[allow(dead_code)]
    env: Env,
}

impl SyncDO {
    /// Append `changes` to the journal as ONE batch. Returns the resulting
    /// version (unchanged when `changes` is empty). The batch is stored as-is —
    /// the server does not process its contents.
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

    /// Journal tail newer than the client's version, strictly ordered. NOTHING
    /// can be read from an uninitialized store — the first client must migrate
    /// its local data in first (/sync/v2/push day-batches + /sync/v2/init).
    async fn v2_pull(&self, body: &serde_json::Value) -> Result<Response> {
        let storage = self.state.storage();
        let last: u64 = storage.get(V2_LAST).await?.unwrap_or(0);
        if !storage.get::<bool>(V2_INIT).await?.unwrap_or(false) {
            return Ok(Response::from_json(
                &serde_json::json!({ "error": "store_not_initialized", "version": last }),
            )?
            .with_status(409));
        }
        let since = body.get("since_version").and_then(|v| v.as_u64()).unwrap_or(0);

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
                // The journal is complete by construction (migration prefix) —
                // a hole is corrupted state, and we fail loudly, not serve gaps.
                None => {
                    return Err(Error::RustError(format!(
                        "sync v2: journal hole at version {v} (last {last})"
                    )))
                }
            }
        }
        Response::from_json(&serde_json::json!({ "version": last, "batches": batches }))
    }

    /// Erase this user's entire journal — the DO is per-user, so «forget this
    /// account» is exactly «drop everything here», including the initialized flag,
    /// so a re-registered account starts from a virgin store.
    async fn v2_wipe(&self) -> Result<Response> {
        self.state.storage().delete_all().await?;
        Response::from_json(&serde_json::json!({ "ok": true }))
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
            (Method::Post, "/sync/v2/push") => {
                let payload: serde_json::Value = req.json().await?;
                self.v2_push(&payload).await
            }
            (Method::Post, "/sync/v2/pull") => {
                let payload: serde_json::Value = req.json().await?;
                self.v2_pull(&payload).await
            }
            (Method::Post, "/internal/user-wipe") => self.v2_wipe().await,
            (Method::Post, "/sync/v2/init") => {
                let payload: serde_json::Value = req.json().await?;
                self.v2_init(&payload).await
            }
            _ => Response::error("Not found", 404),
        }
    }
}

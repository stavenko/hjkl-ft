//! Sync v2: incremental, journaled.
//!
//! The CLIENT forms the change list: every local mutation of a synced store is
//! journaled into the `_outbox` by the tracked `db::put`/`db::delete`. A sync
//! PUSHES the outbox as ONE ordered batch carrying the client's `base_version`;
//! the server appends it to a journal (`version = last + 1`) and applies it to
//! the materialized state. A PULL fetches only the journal tail newer than the
//! client's version and applies it strictly in order (a delete is just an op in
//! the stream). The journal is COMPLETE from the account's genesis, so a zero
//! client simply replays it from the beginning — no snapshot concept at all.
//!
//! Applying remote data uses the `_untracked` db variants and writes only rows
//! that actually differ — an idle sync performs ZERO IndexedDB writes and never
//! disturbs the UI's version signals.

use api_types::*;
use serde::de::DeserializeOwned;
use wasm_bindgen::{JsCast, JsValue};
use wasm_bindgen_futures::JsFuture;

use super::{auth, config, db};

/// POST `body` (JSON) to `{sync_base_url}{path}` with the bearer token and parse
/// the JSON response into `O`. Fails loudly — sync is not allowed to swallow errors.
async fn post_json<O: DeserializeOwned>(path: &str, body: &str) -> Result<O, String> {
    let base = &config::get().sync_base_url;
    if base.is_empty() {
        return Err("sync_base_url is not configured".to_string());
    }
    let url = format!("{base}{path}");
    let token = auth::get_token().ok_or_else(|| "not authenticated".to_string())?;

    let opts = web_sys::RequestInit::new();
    opts.set_method("POST");
    opts.set_body(&JsValue::from_str(body));

    let headers = web_sys::Headers::new().map_err(|e| format!("{e:?}"))?;
    headers.set("Content-Type", "application/json").map_err(|e| format!("{e:?}"))?;
    headers.set("Authorization", &format!("Bearer {token}")).map_err(|e| format!("{e:?}"))?;
    opts.set_headers(&headers);

    let request =
        web_sys::Request::new_with_str_and_init(&url, &opts).map_err(|e| format!("{e:?}"))?;
    let window = web_sys::window().expect("no window");
    // A fetch REJECTION (not an HTTP error) means the sync worker is unreachable —
    // drop its flag immediately so the degraded warning shows without waiting for
    // the next scheduled probe.
    let resp_val = match JsFuture::from(window.fetch_with_request(&request)).await {
        Ok(v) => v,
        Err(e) => {
            super::net::note_failure(super::net::Worker::Sync);
            return Err(format!("{e:?}"));
        }
    };
    let resp: web_sys::Response = resp_val.dyn_into().map_err(|_| "not a Response".to_string())?;

    let text = JsFuture::from(resp.text().map_err(|e| format!("{e:?}"))?)
        .await
        .map_err(|e| format!("{e:?}"))?;
    let text = text.as_string().ok_or("response not a string")?;

    if !resp.ok() {
        return Err(format!("HTTP {}: {}", resp.status(), text));
    }
    serde_json::from_str(&text).map_err(|e| format!("parse error: {e}"))
}

// ── Client version (the data version this device has) ───────────────────────

const META_VERSION: &str = "v2_version";

async fn client_version() -> Option<u64> {
    db::get::<MetaEntry>("_sync_meta", META_VERSION)
        .await
        .and_then(|m| m.value.parse().ok())
}

async fn set_client_version(v: u64) {
    set_meta(META_VERSION, &v.to_string()).await;
}

// ── Wire ↔ local store mapping ───────────────────────────────────────────────

/// Local (store, key) for a wire (store, id). The four `ind_*` stores travel as
/// "ind_days" with the composite `"<indicator>:<date>"` id.
fn local_target(store: &str, id: &str) -> Option<(String, String)> {
    match store {
        "foods" | "diary" | "recipes" | "recipe_ingredients" | "goals" | "profile"
        | "weight_entries" | "step_entries" | "deletions" | "app_flags" => {
            Some((store.to_string(), id.to_string()))
        }
        "ind_days" => {
            let (ind, date) = id.split_once(':')?;
            let s = crate::services::indicators::store_for_indicator(ind)?;
            Some((s.to_string(), date.to_string()))
        }
        _ => None,
    }
}

/// The row to send for an outbox upsert — read FRESH from the local store (so
/// several edits of one row collapse into its latest state). `None` when the
/// row no longer exists (a later delete op in the outbox covers it).
async fn wire_row(store: &str, id: &str) -> Option<serde_json::Value> {
    let (local_store, local_key) = local_target(store, id)?;
    let row: serde_json::Value = db::get(&local_store, &local_key).await?;
    if store == "ind_days" {
        let (ind, date) = id.split_once(':')?;
        return Some(serde_json::json!({
            "id": id,
            "indicator": ind,
            "date": date,
            "value": row.get("value").cloned().unwrap_or(serde_json::json!(0.0)),
            "ratio": row.get("ratio").cloned().unwrap_or(serde_json::Value::Null),
            "computed_at": row.get("computed_at").cloned().unwrap_or(serde_json::json!("")),
        }));
    }
    Some(row)
}

// ── Push: drain the outbox as one ordered batch (CAS on the version) ─────────

/// Push the outbox as one batch built on `base_version = client version`.
/// Returns `Ok(true)` when the server ACCEPTED it (or there was nothing to
/// send); `Ok(false)` when the base was stale — the caller must pull (merge the
/// newer batches) and retry. The outbox is cleared ONLY on acceptance.
async fn push_v2() -> Result<bool, String> {
    let base_version = client_version()
        .await
        .ok_or_else(|| "sync v2: push before bootstrap".to_string())?;
    // `_outbox` keys are zero-padded monotonic seqs → get_all returns mutation order.
    let entries: Vec<db::OutboxEntry> = db::list_all("_outbox").await;
    if entries.is_empty() {
        return Ok(true);
    }

    let mut changes = Vec::with_capacity(entries.len());
    for e in &entries {
        match e.op.as_str() {
            "delete" => changes.push(serde_json::json!({
                "store": e.store, "op": "delete", "id": e.id, "ts": e.ts,
            })),
            _ => {
                if let Some(row) = wire_row(&e.store, &e.id).await {
                    changes.push(serde_json::json!({
                        "store": e.store, "op": "upsert", "row": row, "ts": e.ts,
                    }));
                }
            }
        }
    }

    if !changes.is_empty() {
        let body =
            serde_json::json!({ "base_version": base_version, "changes": changes }).to_string();
        let resp: SyncPushV2Response = post_json("/sync/v2/push", &body).await?;
        if !resp.accepted {
            // Someone pushed after our last pull: merge their batches first.
            return Ok(false);
        }
        set_client_version(resp.version).await;
    }

    // Accepted (or resolved-to-nothing) — clear the drained entries.
    for e in &entries {
        db::delete("_outbox", &e.seq).await;
    }
    set_meta("last_push_at", &chrono::Utc::now().to_rfc3339()).await;
    Ok(true)
}

// ── Pull: apply the journal tail strictly in order ───────────────────────────

/// Pending local (unpushed) mutations per wire (store, id): the latest event
/// time and every outbox seq touching that row. Consulted during the merge.
type PendingMap = std::collections::HashMap<(String, String), (u64, Vec<String>)>;

async fn pending_map() -> PendingMap {
    let mut map: PendingMap = std::collections::HashMap::new();
    for e in db::list_all::<db::OutboxEntry>("_outbox").await {
        let entry = map
            .entry((e.store.clone(), e.id.clone()))
            .or_insert((0, Vec::new()));
        entry.0 = entry.0.max(e.ts);
        entry.1.push(e.seq);
    }
    map
}

/// Automatic conflict resolution for a row touched BOTH remotely and by a
/// pending local mutation, by EVENT TIME: the later event wins (covers the two
/// real cases — a grams edit on the same diary entry on two devices, and an
/// edit vs a deletion of the same entry). `ind_days` keeps first-computation-
/// wins, so there the EARLIER event wins. Ties keep the local change.
fn local_wins(store: &str, local_ts: u64, remote_ts: u64) -> bool {
    match store {
        "ind_days" => local_ts <= remote_ts,
        _ => local_ts >= remote_ts,
    }
}

#[derive(Default)]
struct ApplyCtx {
    flags_touched: bool,
    profile_touched: bool,
    pending: PendingMap,
}

impl ApplyCtx {
    /// Merge gate for one incoming change: true ⇒ apply it (dropping the losing
    /// local pending ops), false ⇒ the local pending change wins, skip it.
    async fn remote_wins(&mut self, store: &str, id: &str, remote_ts: u64) -> bool {
        let key = (store.to_string(), id.to_string());
        let Some((local_ts, seqs)) = self.pending.get(&key).cloned() else {
            return true; // no local pending change → nothing to merge
        };
        if local_wins(store, local_ts, remote_ts) {
            return false;
        }
        // Remote wins: the losing local ops must not be pushed later.
        for seq in &seqs {
            db::delete("_outbox", seq).await;
        }
        self.pending.remove(&key);
        true
    }
}

/// Upsert a wire row into a local store; identical rows are skipped so idle
/// syncs never touch IndexedDB / the UI's version signals. Batches are applied
/// in journal order — the order IS the truth (the server applied them the same
/// way); per-row conflicts were already resolved by the merge gate.
async fn upsert_row(local_store: &str, key: &str, row: &serde_json::Value) -> bool {
    let existing: Option<serde_json::Value> = db::get(local_store, key).await;
    if existing.as_ref() == Some(row) {
        return false;
    }
    db::put_json_untracked(local_store, row).await;
    true
}

async fn apply_upsert(store: &str, row: &serde_json::Value, ctx: &mut ApplyCtx) {
    match store {
        "ind_days" => crate::services::indicators::apply_ind_day(row).await,
        "app_flags" => {
            let Some(key) = row.get("key").and_then(|v| v.as_str()) else {
                leptos::logging::error!("sync v2: app_flags row without key: {row}");
                return;
            };
            if super::app_flags::is_device_local(key) {
                return;
            }
            if upsert_row("app_flags", key, row).await {
                ctx.flags_touched = true;
            }
        }
        "profile" => {
            let Some(key) = row.get("key").and_then(|v| v.as_str()) else {
                leptos::logging::error!("sync v2: profile row without key: {row}");
                return;
            };
            if upsert_row("profile", key, row).await {
                ctx.profile_touched = true;
            }
        }
        "foods" | "diary" | "recipes" | "recipe_ingredients" | "goals" | "weight_entries"
        | "step_entries" | "deletions" => {
            let Some(id) = row.get("id").and_then(|v| v.as_str()) else {
                leptos::logging::error!("sync v2: {store} row without id: {row}");
                return;
            };
            upsert_row(store, id, row).await;
        }
        other => leptos::logging::error!("sync v2: unknown wire store {other:?}"),
    }
}

async fn apply_delete(store: &str, id: &str, ctx: &mut ApplyCtx) {
    if store == "app_flags" && super::app_flags::is_device_local(id) {
        return;
    }
    let Some((local_store, local_key)) = local_target(store, id) else {
        leptos::logging::error!("sync v2: delete for unknown store {store:?}");
        return;
    };
    db::delete_untracked(&local_store, &local_key).await;
    match store {
        "app_flags" => ctx.flags_touched = true,
        "profile" => ctx.profile_touched = true,
        _ => {}
    }
}

/// The wire id of an incoming change (for upserts — extracted from the row).
fn change_id(ch: &SyncChange) -> Option<String> {
    if let Some(id) = &ch.id {
        return Some(id.clone());
    }
    let row = ch.row.as_ref()?;
    match ch.store.as_str() {
        "profile" | "app_flags" => row.get("key").and_then(|v| v.as_str()).map(String::from),
        _ => row.get("id").and_then(|v| v.as_str()).map(String::from),
    }
}

async fn apply_change(ch: &SyncChange, ctx: &mut ApplyCtx) {
    // Merge gate: a row also touched by a pending LOCAL mutation resolves by
    // event time before anything is applied.
    if let Some(id) = change_id(ch) {
        if !ctx.remote_wins(&ch.store, &id, ch.ts).await {
            return;
        }
    }
    match ch.op.as_str() {
        "upsert" => match &ch.row {
            Some(row) => apply_upsert(&ch.store, row, ctx).await,
            None => leptos::logging::error!("sync v2: upsert without row ({})", ch.store),
        },
        "delete" => match &ch.id {
            Some(id) => apply_delete(&ch.store, id, ctx).await,
            None => leptos::logging::error!("sync v2: delete without id ({})", ch.store),
        },
        other => leptos::logging::error!("sync v2: unknown op {other:?}"),
    }
}

async fn pull_v2() -> Result<(), String> {
    let since = client_version().await.unwrap_or(0);
    let body = serde_json::json!({ "since_version": since }).to_string();
    let resp: SyncPullV2Response = post_json("/sync/v2/pull", &body).await?;

    // The journal is the ONLY data source: a zero client replays it from the
    // genesis; everyone else applies just the tail. Strictly in order.
    let mut ctx = ApplyCtx {
        pending: pending_map().await,
        ..Default::default()
    };
    for batch in &resp.batches {
        for ch in &batch.changes {
            apply_change(ch, &mut ctx).await;
        }
    }

    // Refresh the synchronous caches only when their data actually changed.
    if ctx.flags_touched {
        super::app_flags::reload().await;
    }
    if ctx.profile_touched {
        super::profile::hydrate().await;
    }

    // A bootstrap pull must record its version even when it is 0 (a fresh
    // account) — otherwise the client would re-bootstrap on every sync.
    match client_version().await {
        None => set_client_version(resp.version).await,
        Some(cur) if resp.version > cur => set_client_version(resp.version).await,
        _ => {}
    }
    set_meta("last_pull_at", &chrono::Utc::now().to_rfc3339()).await;
    Ok(())
}

// ── Bootstrap: the ONE full-data exchange of a device's lifetime ─────────────

/// Legacy full-state push (v1 endpoint). Used exactly once per device — before
/// it has a version — so its complete local state lands on the server (the v1
/// handler bridges accepted rows into the v2 journal).
async fn push_full_legacy() -> Result<(), String> {
    let payload = SyncPushPayload {
        foods: db::list_all("foods").await,
        diary_entries: db::list_all("diary").await,
        recipes: db::list_all("recipes").await,
        recipe_ingredients: db::list_all("recipe_ingredients").await,
        goals: db::list_all("goals").await,
        profile: db::list_all("profile").await,
        weight_entries: db::list_all("weight_entries").await,
        step_entries: db::list_all("step_entries").await,
        app_flags: db::list_all::<AppFlagRow>("app_flags")
            .await
            .into_iter()
            .filter(|r| !super::app_flags::is_device_local(&r.key) && !r.updated_at.is_empty())
            .collect(),
        ind_days: crate::services::indicators::export_ind_days().await,
        deletions: db::list_all("deletions").await,
    };
    let body = serde_json::to_string(&payload).map_err(|e| e.to_string())?;
    let _resp: SyncPushResponse = post_json("/sync/push", &body).await?;
    Ok(())
}

/// One-time bootstrap for a device without a version: full legacy push (bridged
/// into the journal), then a from-genesis journal replay which sets the version.
async fn ensure_bootstrapped() -> Result<(), String> {
    if client_version().await.is_some() {
        return Ok(());
    }
    push_full_legacy().await?;
    pull_v2().await
}

// ── Public entry points (same names as v1) ───────────────────────────────────

/// One full reconcile: pull (merging newer batches with pending local changes),
/// then push from the fresh base. A CAS rejection (someone pushed in between)
/// loops back through pull; bounded retries fail loudly.
async fn sync_cycle() -> Result<(), String> {
    ensure_bootstrapped().await?;
    pull_v2().await?;
    for _ in 0..4 {
        if push_v2().await? {
            return Ok(());
        }
        pull_v2().await?;
    }
    Err("sync v2: push kept conflicting after retries".to_string())
}

/// Reconcile with the server (launch / foreground / login).
pub async fn sync_now() -> Result<(), String> {
    sync_cycle().await
}

/// Fire-and-forget sync after a local mutation. Logs (does not hide) failures.
pub fn push_background() {
    leptos::spawn_local(async {
        if let Err(e) = sync_cycle().await {
            leptos::logging::warn!("Background sync push failed: {e}");
        }
    });
}

/// Fire-and-forget full reconcile. Used at launch and when the app regains focus.
pub fn sync_now_background() {
    leptos::spawn_local(async {
        if let Err(e) = sync_now().await {
            leptos::logging::warn!("Sync reconcile failed: {e}");
        }
    });
}

pub async fn is_empty() -> bool {
    db::count("foods").await == 0 && db::count("goals").await == 0
}

#[derive(serde::Serialize, serde::Deserialize)]
struct MetaEntry {
    key: String,
    value: String,
}

async fn set_meta(key: &str, value: &str) {
    let entry = MetaEntry {
        key: key.to_string(),
        value: value.to_string(),
    };
    db::put("_sync_meta", &entry).await;
}

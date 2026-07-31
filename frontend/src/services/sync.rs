//! Sync v2: incremental, journaled.
//!
//! The CLIENT forms the change list: every local mutation of a synced store is
//! journaled into the `_outbox` by the tracked `db::put`/`db::delete`. A sync
//! PUSHES the outbox as ONE ordered batch carrying the client's `base_version`;
//! the server appends it to a journal (`version = last + 1`) and applies it to
//! the materialized state. A PULL fetches only the journal tail newer than the
//! client's version and applies it strictly in order (a delete is just an op in
//! the stream). Full data travels exactly once — to a zero client (snapshot).
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

// ── Push: drain the outbox as one ordered batch ──────────────────────────────

async fn push_v2() -> Result<(), String> {
    let base_version = client_version()
        .await
        .ok_or_else(|| "sync v2: push before bootstrap".to_string())?;
    // `_outbox` keys are zero-padded monotonic seqs → get_all returns mutation order.
    let entries: Vec<db::OutboxEntry> = db::list_all("_outbox").await;
    if entries.is_empty() {
        return Ok(());
    }

    let mut changes = Vec::with_capacity(entries.len());
    for e in &entries {
        match e.op.as_str() {
            "delete" => changes.push(serde_json::json!({
                "store": e.store, "op": "delete", "id": e.id,
            })),
            _ => {
                if let Some(row) = wire_row(&e.store, &e.id).await {
                    changes.push(serde_json::json!({
                        "store": e.store, "op": "upsert", "row": row,
                    }));
                }
            }
        }
    }

    if !changes.is_empty() {
        let body =
            serde_json::json!({ "base_version": base_version, "changes": changes }).to_string();
        let resp: SyncPushV2Response = post_json("/sync/v2/push", &body).await?;
        // Advance only when NOTHING landed between our version and our batch —
        // otherwise the next pull must fetch the intervening batches (our own
        // batch coming back with them is an idempotent no-op).
        if resp.version == base_version + 1 {
            set_client_version(resp.version).await;
        }
    }

    // Sent (or resolved-to-nothing) — clear the drained entries.
    for e in &entries {
        db::delete("_outbox", &e.seq).await;
    }
    set_meta("last_push_at", &chrono::Utc::now().to_rfc3339()).await;
    Ok(())
}

// ── Pull: apply the journal tail (or a bootstrap snapshot) in order ──────────

#[derive(Default)]
struct ApplyCtx {
    flags_touched: bool,
    profile_touched: bool,
}

/// Upsert a wire row into a local store; returns true when it actually wrote.
/// Applies the SAME acceptance rule the server's materialization uses (LWW by
/// `updated_at`) — journal batches may carry rows the server itself rejected
/// (stale pushes, our own echo), and blind application would diverge from the
/// server state. Equal-or-older rows are skipped, which also makes idle syncs
/// write nothing.
async fn upsert_row(local_store: &str, key: &str, row: &serde_json::Value) -> bool {
    let existing: Option<serde_json::Value> = db::get(local_store, key).await;
    if let Some(cur) = &existing {
        let inc = row.get("updated_at").and_then(|v| v.as_str()).unwrap_or("");
        let cur_ts = cur.get("updated_at").and_then(|v| v.as_str()).unwrap_or("");
        if inc <= cur_ts {
            return false;
        }
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

async fn apply_change(ch: &SyncChange, ctx: &mut ApplyCtx) {
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

async fn apply_snapshot(snapshot: &serde_json::Value, ctx: &mut ApplyCtx) {
    let Some(map) = snapshot.as_object() else {
        leptos::logging::error!("sync v2: snapshot is not an object");
        return;
    };
    for (store, rows) in map {
        let Some(rows) = rows.as_array() else { continue };
        for row in rows {
            apply_upsert(store, row, ctx).await;
        }
    }
}

async fn pull_v2() -> Result<(), String> {
    let since = client_version().await.unwrap_or(0);
    let body = serde_json::json!({ "since_version": since }).to_string();
    let resp: SyncPullV2Response = post_json("/sync/v2/pull", &body).await?;

    let mut ctx = ApplyCtx::default();
    if let Some(snapshot) = &resp.snapshot {
        apply_snapshot(snapshot, &mut ctx).await;
    } else {
        for batch in &resp.batches {
            for ch in &batch.changes {
                apply_change(ch, &mut ctx).await;
            }
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
/// into the journal), then a snapshot pull which sets the version.
async fn ensure_bootstrapped() -> Result<(), String> {
    if client_version().await.is_some() {
        return Ok(());
    }
    push_full_legacy().await?;
    pull_v2().await
}

// ── Public entry points (same names as v1) ───────────────────────────────────

/// Reconcile with the server: push local changes, then pull others' changes.
pub async fn sync_now() -> Result<(), String> {
    ensure_bootstrapped().await?;
    push_v2().await?;
    pull_v2().await
}

/// Fire-and-forget push after a local mutation. Logs (does not hide) failures.
pub fn push_background() {
    leptos::spawn_local(async {
        let res = async {
            ensure_bootstrapped().await?;
            push_v2().await
        }
        .await;
        if let Err(e) = res {
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

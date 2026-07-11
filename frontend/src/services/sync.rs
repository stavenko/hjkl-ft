use api_types::*;
use serde::de::DeserializeOwned;
use wasm_bindgen::{JsCast, JsValue};
use wasm_bindgen_futures::JsFuture;

use super::{auth, config, db, local};

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

/// Pull the full server dataset and replace local synced stores with it.
///
/// Ordering note: callers push BEFORE pulling (see [`sync_now`]), so this
/// clear-and-repopulate never loses unpushed local state. Replacing (rather than
/// merging) is what propagates deletions — the server's `dump` omits soft-deleted
/// diary rows, so they vanish locally too.
pub async fn pull_full_dump() -> Result<(), String> {
    let dump: SyncDumpResponse = post_json("/sync/dump", "{}").await?;

    // MERGE, never clear-and-replace: upsert server rows and KEEP local rows the
    // server didn't return. A wholesale clear could wipe local data on an empty or
    // partial dump. Deletions are not inferred from absence — they are explicit
    // records (below) applied on every device.
    merge_store("foods", &dump.foods).await;
    merge_store("diary", &dump.diary_entries).await;
    merge_store("recipes", &dump.recipes).await;
    merge_store("recipe_ingredients", &dump.recipe_ingredients).await;
    merge_store("goals", &dump.goals).await;
    merge_store("story", &dump.story).await;
    merge_store("profile", &dump.profile).await;
    merge_store("weight_entries", &dump.weight_entries).await;
    merge_store("step_entries", &dump.step_entries).await;

    // Deletion log: persist the records, then apply them — removing the targets
    // locally even though the server still re-sends the (un-deleted) entities.
    merge_store("deletions", &dump.deletions).await;
    local::apply_deletions().await;

    // A pulled profile row lands in IndexedDB above; refresh the synchronous
    // in-memory cache so getters see a remote update without a relaunch.
    super::profile::hydrate().await;

    set_meta("last_pull_at", &chrono::Utc::now().to_rfc3339()).await;
    Ok(())
}

/// Upsert the server rows into a local store (put each by its key). Local rows
/// absent from the dump are left intact — removals go through the deletion log.
async fn merge_store<T: serde::Serialize>(store: &str, rows: &[T]) {
    for row in rows {
        db::put(store, row).await;
    }
}

pub async fn push_to_server() -> Result<(), String> {
    let payload = SyncPushPayload {
        foods: db::list_all("foods").await,
        diary_entries: db::list_all("diary").await,
        recipes: db::list_all("recipes").await,
        recipe_ingredients: db::list_all("recipe_ingredients").await,
        goals: db::list_all("goals").await,
        story: db::list_all("story").await,
        profile: db::list_all("profile").await,
        weight_entries: db::list_all("weight_entries").await,
        step_entries: db::list_all("step_entries").await,
        deletions: db::list_all("deletions").await,
    };
    let body = serde_json::to_string(&payload).map_err(|e| e.to_string())?;

    let _resp: SyncPushResponse = post_json("/sync/push", &body).await?;

    set_meta("last_push_at", &chrono::Utc::now().to_rfc3339()).await;
    Ok(())
}

/// Reconcile with the server: push local state first (so the server has every
/// local change under last-writer-wins), then pull the merged result. This is
/// the launch / foreground entry point — it lets a device see changes (including
/// deletions) made on other devices.
pub async fn sync_now() -> Result<(), String> {
    push_to_server().await?;
    pull_full_dump().await
}

/// Fire-and-forget push after a local mutation. Logs (does not hide) failures.
pub fn push_background() {
    leptos::spawn_local(async {
        if let Err(e) = push_to_server().await {
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

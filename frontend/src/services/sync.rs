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
    let resp_val = JsFuture::from(window.fetch_with_request(&request))
        .await
        .map_err(|e| format!("{e:?}"))?;
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

    db::clear("foods").await;
    for food in &dump.foods {
        db::put("foods", food).await;
    }

    db::clear("diary").await;
    for entry in &dump.diary_entries {
        db::put("diary", entry).await;
    }

    db::clear("recipes").await;
    for recipe in &dump.recipes {
        db::put("recipes", recipe).await;
    }

    db::clear("recipe_ingredients").await;
    for ing in &dump.recipe_ingredients {
        db::put("recipe_ingredients", ing).await;
    }

    db::clear("goals").await;
    for goal in &dump.goals {
        db::put("goals", goal).await;
    }

    db::clear("story").await;
    for flag in &dump.story {
        db::put("story", flag).await;
    }

    db::clear("weight_entries").await;
    for w in &dump.weight_entries {
        db::put("weight_entries", w).await;
    }

    db::clear("step_entries").await;
    for s in &dump.step_entries {
        db::put("step_entries", s).await;
    }

    set_meta("last_pull_at", &chrono::Utc::now().to_rfc3339()).await;
    Ok(())
}

pub async fn push_to_server() -> Result<(), String> {
    let payload = SyncPushPayload {
        foods: db::list_all("foods").await,
        diary_entries: db::list_all("diary").await,
        recipes: db::list_all("recipes").await,
        recipe_ingredients: db::list_all("recipe_ingredients").await,
        goals: db::list_all("goals").await,
        story: db::list_all("story").await,
        weight_entries: db::list_all("weight_entries").await,
        step_entries: db::list_all("step_entries").await,
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

use api_types::*;

use super::{api, db};

pub async fn pull_full_dump() -> Result<(), String> {
    let dump: SyncDumpResponse = api::post("/sync/dump", &())
        .await
        .map_err(|_| "failed to fetch dump from server".to_string())?;

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
    };

    let _resp: SyncPushResponse = api::post("/sync/push", &payload)
        .await
        .map_err(|_| "failed to push to server".to_string())?;

    set_meta("last_push_at", &chrono::Utc::now().to_rfc3339()).await;

    Ok(())
}

pub fn push_background() {
    leptos::spawn_local(async {
        if let Err(e) = push_to_server().await {
            leptos::logging::warn!("Background sync push failed: {e}");
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

use std::cell::RefCell;
use std::collections::HashMap;

use leptos::*;
use rexie::{ObjectStore, Rexie, TransactionMode};
use serde::{de::DeserializeOwned, Serialize};
use wasm_bindgen::JsValue;

thread_local! {
    static DB: RefCell<Option<Rexie>> = RefCell::new(None);
    // Name of the currently-active database, so the connection can be REOPENED
    // after iOS force-closes it on a backgrounded PWA (see [`reopen`]).
    static DB_NAME: RefCell<Option<String>> = RefCell::new(None);
    static STORE_VERSIONS: RefCell<HashMap<&'static str, RwSignal<u32>>> = RefCell::new(HashMap::new());
}

/// Reopen the active database connection. iOS closes a PWA's IndexedDB connection
/// while it's backgrounded; the cached `Rexie` then wedges (transactions hang or
/// error), so writes silently fail and reads return nothing until a full reload.
/// Call this on resume (foreground) to get a live connection, then re-query.
pub async fn reopen() {
    let Some(name) = DB_NAME.with(|c| c.borrow().clone()) else { return };
    let fresh = open(&name).await;
    DB.with(|cell| cell.replace(Some(fresh)));
    bump_all();
}

pub fn version(store_name: &'static str) -> RwSignal<u32> {
    STORE_VERSIONS.with(|cell| {
        let map = cell.borrow();
        *map.get(store_name).expect("db::init() must be called before db::version()")
    })
}

fn bump(store_name: &str) {
    STORE_VERSIONS.with(|cell| {
        let map = cell.borrow();
        if let Some(sig) = map.get(store_name) {
            sig.update(|v| *v += 1);
        }
    });
}

/// The legacy, device-global database. Used before login (no user identity yet)
/// and as the one-time migration source for users created before per-user scoping.
const BOOTSTRAP_DB: &str = "hjkl-ft";
const DB_VERSION: u32 = 13;

/// Every object store, in a single list. `_sync_meta` carries sync cursors and
/// `app_flags` holds per-user UI flags (onboarding/subscription); neither is
/// synced. The rest hold user data. Used for migration copy/clear and emptiness
/// checks.
const ALL_STORES: &[&str] = &[
    "foods", "diary", "recipes", "recipe_ingredients",
    "goals", "food_drafts", "weight_entries", "step_entries",
    "progress_photos", "summaries", "chat", "story", "profile", "deletions", "_sync_meta",
    "app_flags",
    "support_messages", "support_outbox", "support_meta",
];

/// Per-user database name. Each account gets its own IndexedDB so a different
/// user signing in on the same device never sees or pushes the previous user's
/// data.
fn user_db_name(user_id: &str) -> String {
    format!("hjkl-ft-{user_id}")
}

/// Build the schema (identical for every database name we open).
fn builder(name: &str) -> rexie::RexieBuilder {
    Rexie::builder(name)
        .version(DB_VERSION)
        .add_object_store(
            ObjectStore::new("foods")
                .key_path("id")
                .add_index(rexie::Index::new("name", "name"))
                .add_index(rexie::Index::new("archived", "archived"))
                .add_index(rexie::Index::new("updated_at", "updated_at")),
        )
        .add_object_store(
            ObjectStore::new("diary")
                .key_path("id")
                .add_index(rexie::Index::new("date", "date"))
                .add_index(rexie::Index::new("food_id", "food_id"))
                .add_index(rexie::Index::new("updated_at", "updated_at")),
        )
        .add_object_store(
            ObjectStore::new("recipes")
                .key_path("id")
                .add_index(rexie::Index::new("name", "name"))
                .add_index(rexie::Index::new("updated_at", "updated_at")),
        )
        .add_object_store(
            ObjectStore::new("recipe_ingredients")
                .key_path("id")
                .add_index(rexie::Index::new("recipe_id", "recipe_id"))
                .add_index(rexie::Index::new("food_id", "food_id"))
                .add_index(rexie::Index::new("updated_at", "updated_at")),
        )
        .add_object_store(
            ObjectStore::new("goals")
                .key_path("id")
                .add_index(rexie::Index::new("nutrient", "nutrient"))
                .add_index(rexie::Index::new("updated_at", "updated_at")),
        )
        .add_object_store(
            ObjectStore::new("food_drafts")
                .key_path("id")
                .add_index(rexie::Index::new("food_id", "food_id"))
                .add_index(rexie::Index::new("created_at", "created_at")),
        )
        .add_object_store(
            ObjectStore::new("weight_entries")
                .key_path("id")
                .add_index(rexie::Index::new("date", "date"))
                .add_index(rexie::Index::new("updated_at", "updated_at")),
        )
        .add_object_store(
            ObjectStore::new("step_entries")
                .key_path("id")
                .add_index(rexie::Index::new("date", "date"))
                .add_index(rexie::Index::new("updated_at", "updated_at")),
        )
        .add_object_store(
            ObjectStore::new("progress_photos")
                .key_path("id")
                .add_index(rexie::Index::new("pose", "pose"))
                .add_index(rexie::Index::new("created_at", "created_at")),
        )
        // Daily / weekly AI summaries, keyed "day:YYYY-MM-DD" / "week:YYYY-MM-DD".
        .add_object_store(ObjectStore::new("summaries").key_path("id"))
        // Support-chat messages, one record per message, keyed by uuid v7 "id".
        .add_object_store(
            ObjectStore::new("chat")
                .key_path("id")
                .add_index(rexie::Index::new("created_at", "created_at")),
        )
        .add_object_store(ObjectStore::new("story").key_path("key"))
        // Synced user profile, a keyed singleton (one row, key "profile").
        .add_object_store(ObjectStore::new("profile").key_path("key"))
        // Explicit deletion records (tombstones), synced and applied on every device.
        .add_object_store(ObjectStore::new("deletions").key_path("id"))
        .add_object_store(ObjectStore::new("_sync_meta").key_path("key"))
        // Per-user UI flags (onboarding dismissals, paywall-skip date, cached
        // subscription status). Not synced — these are per-user-per-device.
        .add_object_store(ObjectStore::new("app_flags").key_path("key"))
        // Live support thread (separate server from the AI `chat` store). Not
        // synced — per-user-per-device cache, cursor, and optimistic outbox.
        .add_object_store(ObjectStore::new("support_messages").key_path("seq"))
        .add_object_store(ObjectStore::new("support_outbox").key_path("client_id"))
        .add_object_store(ObjectStore::new("support_meta").key_path("key"))
        // Per-indicator per-day aggregate cache (one store per indicator, keyed by
        // date). Derived, per-device, NOT synced — recomputed on demand from the
        // diary/foods when a day is missing (see `services::indicators` cache).
        .add_object_store(ObjectStore::new("ind_protein").key_path("date"))
        .add_object_store(ObjectStore::new("ind_veg_fruit").key_path("date"))
}

async fn open(name: &str) -> Rexie {
    builder(name)
        .build()
        .await
        .expect("failed to open IndexedDB")
}

/// Open the bootstrap database and create the reactive version signals. The
/// active database is switched to the per-user one by [`activate_for_user`] once
/// the signed-in user is known (at launch, and on login / pairing).
pub async fn init() {
    let rexie = open(BOOTSTRAP_DB).await;
    DB.with(|cell| cell.replace(Some(rexie)));
    DB_NAME.with(|c| c.replace(Some(BOOTSTRAP_DB.to_string())));

    STORE_VERSIONS.with(|cell| {
        let mut map = cell.borrow_mut();
        for &name in ALL_STORES {
            map.entry(name).or_insert_with(|| create_rw_signal(0u32));
        }
    });

    // Hydrate the profile cache off the bootstrap DB for the signed-out path
    // (`activate_for_user` re-hydrates once a user's database is swapped in).
    crate::services::profile::migrate_from_local_storage().await;
    crate::services::profile::hydrate().await;
}

/// Switch the active database to this user's per-user IndexedDB.
///
/// When `migrate_bootstrap` is set and the user's database is still empty, the
/// legacy shared `hjkl-ft` database is copied in and then cleared — a one-time
/// migration for accounts created before per-user scoping (it also rescues
/// local-only stores like `progress_photos`, which sync never carries). Pass
/// `migrate_bootstrap=true` ONLY for the already-signed-in user at launch, where
/// the bootstrap data is attributable to them; on an explicit login it MUST be
/// false so a new account never inherits the previous user's leftover data.
pub async fn activate_for_user(user_id: &str, migrate_bootstrap: bool) {
    let target = open(&user_db_name(user_id)).await;

    if migrate_bootstrap && is_data_empty(&target).await {
        let bootstrap = open(BOOTSTRAP_DB).await;
        if !is_data_empty(&bootstrap).await {
            copy_all(&bootstrap, &target).await;
            clear_db(&bootstrap).await;
        }
    }

    DB.with(|cell| cell.replace(Some(target)));
    DB_NAME.with(|c| c.replace(Some(user_db_name(user_id))));

    // One-time backfill of the legacy localStorage profile into the synced
    // `profile` store (no-op if a row already exists), then hydrate the in-memory
    // cache so synchronous profile getters read the active user's profile.
    crate::services::profile::migrate_from_local_storage().await;
    crate::services::profile::hydrate().await;

    bump_all();
}

/// True when no data store holds any row (`_sync_meta` is ignored — cursors are
/// not user data).
async fn is_data_empty(db: &Rexie) -> bool {
    for &store in ALL_STORES {
        if store == "_sync_meta" {
            continue;
        }
        let tx = db
            .transaction(&[store], TransactionMode::ReadOnly)
            .expect("failed to create transaction");
        let count = tx.store(store).expect("store not found").count(None).await.expect("count failed");
        if count > 0 {
            return false;
        }
    }
    true
}

/// Copy every row of every store from `src` into `dst` (raw values, by key path).
async fn copy_all(src: &Rexie, dst: &Rexie) {
    for &store in ALL_STORES {
        let rtx = src
            .transaction(&[store], TransactionMode::ReadOnly)
            .expect("failed to create transaction");
        let rows = rtx.store(store).expect("store not found").get_all(None, None).await.expect("get_all failed");
        if rows.is_empty() {
            continue;
        }
        let wtx = dst
            .transaction(&[store], TransactionMode::ReadWrite)
            .expect("failed to create transaction");
        let wstore = wtx.store(store).expect("store not found");
        for val in &rows {
            wstore.put(val, None).await.expect("put failed");
        }
        wtx.done().await.expect("transaction failed");
    }
}

/// Clear every store of a database (used to wipe the bootstrap DB after its data
/// has been migrated into a per-user database).
async fn clear_db(db: &Rexie) {
    for &store in ALL_STORES {
        let tx = db
            .transaction(&[store], TransactionMode::ReadWrite)
            .expect("failed to create transaction");
        tx.store(store).expect("store not found").clear().await.expect("clear failed");
        tx.done().await.expect("transaction failed");
    }
}

/// Bump every store's version signal — call after swapping the active database so
/// reactive readers re-query against the new data.
fn bump_all() {
    STORE_VERSIONS.with(|cell| {
        for sig in cell.borrow().values() {
            sig.update(|v| *v += 1);
        }
    });
}

fn with_db<F, R>(f: F) -> R
where
    F: FnOnce(&Rexie) -> R,
{
    DB.with(|cell| {
        let borrow = cell.borrow();
        let db = borrow.as_ref().expect("DB not initialized");
        f(db)
    })
}

pub async fn put<T: Serialize>(store_name: &str, value: &T) {
    let tx = with_db(|db| {
        db.transaction(&[store_name], TransactionMode::ReadWrite)
            .expect("failed to create transaction")
    });
    let store = tx.store(store_name).expect("store not found");
    let js_val = serde_wasm_bindgen::to_value(value).expect("serialize failed");
    store.put(&js_val, None).await.expect("put failed");
    tx.done().await.expect("transaction failed");
    bump(store_name);
}

pub async fn get<T: DeserializeOwned>(store_name: &str, id: &str) -> Option<T> {
    let tx = with_db(|db| {
        db.transaction(&[store_name], TransactionMode::ReadOnly)
            .expect("failed to create transaction")
    });
    let store = tx.store(store_name).expect("store not found");
    let key = JsValue::from_str(id);
    let result = store.get(key).await.expect("get failed");
    result.map(|js_val| serde_wasm_bindgen::from_value(js_val).expect("deserialize failed"))
}

pub async fn delete(store_name: &str, id: &str) {
    let tx = with_db(|db| {
        db.transaction(&[store_name], TransactionMode::ReadWrite)
            .expect("failed to create transaction")
    });
    let store = tx.store(store_name).expect("store not found");
    let key = JsValue::from_str(id);
    store.delete(key).await.expect("delete failed");
    tx.done().await.expect("transaction failed");
    bump(store_name);
}

pub async fn list_all<T: DeserializeOwned>(store_name: &str) -> Vec<T> {
    let tx = with_db(|db| {
        db.transaction(&[store_name], TransactionMode::ReadOnly)
            .expect("failed to create transaction")
    });
    let store = tx.store(store_name).expect("store not found");
    let entries = store.get_all(None, None).await.expect("get_all failed");
    entries
        .into_iter()
        .map(|val| serde_wasm_bindgen::from_value(val).expect("deserialize failed"))
        .collect()
}

pub async fn list_by_index<T: DeserializeOwned>(
    store_name: &str,
    index_name: &str,
    value: &str,
) -> Vec<T> {
    let tx = with_db(|db| {
        db.transaction(&[store_name], TransactionMode::ReadOnly)
            .expect("failed to create transaction")
    });
    let store = tx.store(store_name).expect("store not found");
    let index = store.index(index_name).expect("index not found");
    let key = JsValue::from_str(value);
    let key_range = rexie::KeyRange::only(&key).expect("key range failed");
    let entries = index
        .get_all(Some(key_range), None)
        .await
        .expect("index query failed");
    entries
        .into_iter()
        .map(|val| serde_wasm_bindgen::from_value(val).expect("deserialize failed"))
        .collect()
}

pub async fn count(store_name: &str) -> u32 {
    let tx = with_db(|db| {
        db.transaction(&[store_name], TransactionMode::ReadOnly)
            .expect("failed to create transaction")
    });
    let store = tx.store(store_name).expect("store not found");
    store.count(None).await.expect("count failed")
}

pub async fn wipe_all() {
    let stores = ["foods", "diary", "recipes", "recipe_ingredients", "goals", "food_drafts", "weight_entries", "step_entries", "chat", "support_messages", "support_outbox", "support_meta", "story", "profile", "deletions", "_sync_meta"];
    for store in stores {
        clear(store).await;
    }
}

pub async fn clear(store_name: &str) {
    let tx = with_db(|db| {
        db.transaction(&[store_name], TransactionMode::ReadWrite)
            .expect("failed to create transaction")
    });
    let store = tx.store(store_name).expect("store not found");
    store.clear().await.expect("clear failed");
    tx.done().await.expect("transaction failed");
    bump(store_name);
}

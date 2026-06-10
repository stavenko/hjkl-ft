use std::cell::RefCell;

use rexie::{ObjectStore, Rexie, TransactionMode};
use serde::{de::DeserializeOwned, Serialize};
use wasm_bindgen::JsValue;

thread_local! {
    static DB: RefCell<Option<Rexie>> = RefCell::new(None);
}

// TODO: scope DB by user_id — change name to "hjkl-ft-{user_id}" so each user
// gets their own IndexedDB. After login, reinitialize DB with the user's ID.
pub async fn init() {
    let rexie = Rexie::builder("hjkl-ft")
        .version(2)
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
        .add_object_store(ObjectStore::new("_sync_meta").key_path("key"))
        .build()
        .await
        .expect("failed to open IndexedDB");

    DB.with(|cell| cell.replace(Some(rexie)));
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
    let stores = ["foods", "diary", "recipes", "recipe_ingredients", "goals", "_sync_meta"];
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
}

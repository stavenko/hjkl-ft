//! Per-user, per-device UI flags — onboarding dismissals, the paywall-skip date,
//! and the cached subscription status. Stored in the active user's IndexedDB
//! `app_flags` store rather than device-global localStorage, so a different
//! account signing in on the same device does NOT inherit the previous user's
//! onboarding/subscription state.
//!
//! A synchronous in-memory cache backs the reads: the overlay flow's `needs_*`
//! checks run before any `await`, so they cannot do an async IndexedDB read. The
//! cache is reloaded from the active database whenever it is switched (launch,
//! login, pairing) via [`activate`]; writes update the cache immediately and
//! persist to the database in the background.
//!
//! NOT stored here (they must be readable before the user's database is known):
//! the identity keys `user_id`/`auth_token`/`token_id`, and the pre-login
//! `pwa_dismissed` flag — both stay in localStorage.

use std::cell::RefCell;
use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::services::db;

thread_local! {
    static CACHE: RefCell<HashMap<String, String>> = RefCell::new(HashMap::new());
}

#[derive(Serialize, Deserialize)]
struct FlagRow {
    key: String,
    value: String,
}

/// Legacy localStorage keys migrated into `app_flags` on the first launch after
/// this move (then deleted from localStorage). One-time and idempotent.
const LEGACY_KEYS: &[&str] = &[
    "push_subscribed",
    "push_onboarding_dismissed",
    "paywall_skipped_date",
    "ft_subscription",
];

fn local_storage() -> Option<web_sys::Storage> {
    web_sys::window()?.local_storage().ok().flatten()
}

/// Reload the cache from the active database, then migrate any leftover legacy
/// localStorage flags in. Call after every active-database switch.
pub async fn activate() {
    reload().await;
    migrate_legacy().await;
}

/// Reload the in-memory cache from the active database's `app_flags` store.
pub async fn reload() {
    let rows: Vec<FlagRow> = db::list_all("app_flags").await;
    CACHE.with(|c| {
        let mut m = c.borrow_mut();
        m.clear();
        for r in rows {
            m.insert(r.key, r.value);
        }
    });
}

/// One-time migration: move the legacy device-global localStorage flags into the
/// active user's `app_flags` store, then remove them from localStorage.
async fn migrate_legacy() {
    let Some(ls) = local_storage() else { return };
    for &key in LEGACY_KEYS {
        // Already in the per-user store → just drop the stale localStorage copy.
        if CACHE.with(|c| c.borrow().contains_key(key)) {
            let _ = ls.remove_item(key);
            continue;
        }
        if let Ok(Some(value)) = ls.get_item(key) {
            CACHE.with(|c| {
                c.borrow_mut().insert(key.to_string(), value.clone());
            });
            put(key, &value).await;
            let _ = ls.remove_item(key);
        }
    }
}

/// Read a flag value (synchronous, from the cache).
pub fn get(key: &str) -> Option<String> {
    CACHE.with(|c| c.borrow().get(key).cloned())
}

/// Read a boolean flag (`"true"` ⇒ true; absent/other ⇒ false).
pub fn get_bool(key: &str) -> bool {
    get(key).as_deref() == Some("true")
}

/// Set a flag: update the cache immediately (the in-session source of truth) and
/// persist to the database in the background.
pub fn set(key: &str, value: &str) {
    CACHE.with(|c| {
        c.borrow_mut().insert(key.to_string(), value.to_string());
    });
    let (k, v) = (key.to_string(), value.to_string());
    leptos::spawn_local(async move { put(&k, &v).await });
}

/// Set a boolean flag; `false` removes it (matching the old "present ⇒ true"
/// localStorage convention).
pub fn set_bool(key: &str, value: bool) {
    if value {
        set(key, "true");
    } else {
        remove(key);
    }
}

/// Remove a flag from the cache and the database.
pub fn remove(key: &str) {
    CACHE.with(|c| {
        c.borrow_mut().remove(key);
    });
    let k = key.to_string();
    leptos::spawn_local(async move { db::delete("app_flags", &k).await });
}

async fn put(key: &str, value: &str) {
    db::put("app_flags", &FlagRow { key: key.to_string(), value: value.to_string() }).await;
}

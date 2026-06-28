//! The user profile (biological sex, height, birth year), kept as a SYNCED
//! keyed-singleton row in the `profile` IndexedDB store (one row, key
//! "profile"), merged last-writer-wins by `updated_at` across devices — exactly
//! like the `story` flags.
//!
//! Reads stay SYNCHRONOUS via an in-memory cache (so the existing sync callers
//! in story/weight-modal/settings don't have to await). The cache is hydrated by
//! [`hydrate`] after every active-database switch (launch, login, pairing) —
//! before any reader runs. Writes read-modify-write the cache row, stamp
//! `updated_at`, persist to IndexedDB, and push to the server in the background.

use std::cell::RefCell;

use api_types::ProfileRow;

use crate::services::{db, sync};

/// The singleton row key.
const PROFILE_KEY: &str = "profile";

/// Legacy device-global localStorage keys, migrated once into the synced row.
const KEY_SEX: &str = "profile_sex";
const KEY_HEIGHT: &str = "profile_height_cm";

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Sex {
    Male,
    Female,
}

/// The overall goal of the course. Defaults to weight loss; the user can switch
/// to maintenance only after the relevant chapter unlocks.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CourseGoal {
    Lose,
    Maintain,
}

thread_local! {
    static CACHE: RefCell<Option<ProfileRow>> = const { RefCell::new(None) };
}

fn local_storage() -> Option<web_sys::Storage> {
    web_sys::window().and_then(|w| w.local_storage().ok().flatten())
}

/// Load the profile row from IndexedDB into the synchronous in-memory cache.
/// Called after the active database is switched (launch / login / pairing).
pub async fn hydrate() {
    let row = db::get::<ProfileRow>("profile", PROFILE_KEY).await;
    CACHE.with(|c| *c.borrow_mut() = row);
}

/// Read the cached row (a clone), or a fresh empty row keyed "profile".
fn row_or_default() -> ProfileRow {
    CACHE.with(|c| c.borrow().clone()).unwrap_or(ProfileRow {
        key: PROFILE_KEY.to_string(),
        sex: None,
        height_cm: None,
        birth_year: None,
        goal: None,
        updated_at: String::new(),
    })
}

/// Read-modify-write: apply `mutate` to the current row, stamp `updated_at`,
/// update the cache, persist to IndexedDB, and push in the background.
fn write(mutate: impl FnOnce(&mut ProfileRow)) {
    let mut row = row_or_default();
    mutate(&mut row);
    row.updated_at = chrono::Utc::now().to_rfc3339();
    CACHE.with(|c| *c.borrow_mut() = Some(row.clone()));
    leptos::spawn_local(async move {
        db::put("profile", &row).await;
        sync::push_background();
    });
}

pub fn get_sex() -> Option<Sex> {
    CACHE.with(|c| {
        c.borrow().as_ref().and_then(|r| match r.sex.as_deref() {
            Some("male") => Some(Sex::Male),
            Some("female") => Some(Sex::Female),
            _ => None,
        })
    })
}

pub fn set_sex(sex: Sex) {
    let v = match sex {
        Sex::Male => "male",
        Sex::Female => "female",
    };
    write(|r| r.sex = Some(v.to_string()));
}

/// The user's height in centimetres, if set (and a positive number).
pub fn get_height_cm() -> Option<f64> {
    CACHE.with(|c| c.borrow().as_ref().and_then(|r| r.height_cm).filter(|h| *h > 0.0))
}

/// Store the height (cm). A non-positive value clears it.
pub fn set_height_cm(cm: f64) {
    write(|r| r.height_cm = if cm > 0.0 { Some(cm) } else { None });
}

/// The user's year of birth, if set and within a sane range.
pub fn get_birth_year() -> Option<i32> {
    let current_year = chrono::Utc::now().format("%Y").to_string().parse::<i32>().unwrap_or(2026);
    CACHE.with(|c| {
        c.borrow()
            .as_ref()
            .and_then(|r| r.birth_year)
            .filter(|y| (1900..=current_year).contains(y))
    })
}

/// Store the year of birth. A value of 0 (or out of range) clears it.
pub fn set_birth_year(year: i32) {
    let current_year = chrono::Utc::now().format("%Y").to_string().parse::<i32>().unwrap_or(2026);
    write(|r| r.birth_year = if (1900..=current_year).contains(&year) { Some(year) } else { None });
}

/// The course goal. Defaults to `Lose` when unset.
pub fn get_goal() -> CourseGoal {
    CACHE.with(|c| {
        match c.borrow().as_ref().and_then(|r| r.goal.as_deref()) {
            Some("maintain") => CourseGoal::Maintain,
            _ => CourseGoal::Lose,
        }
    })
}

/// Store the course goal.
pub fn set_goal(goal: CourseGoal) {
    let v = match goal {
        CourseGoal::Lose => "lose",
        CourseGoal::Maintain => "maintain",
    };
    write(|r| r.goal = Some(v.to_string()));
}

/// Body Mass Index = weight(kg) / height(m)². `None` if height is not a positive
/// value. Used as a coarse read on how much of the body mass is fat.
pub fn bmi(weight_kg: f64, height_cm: f64) -> Option<f64> {
    if height_cm <= 0.0 {
        return None;
    }
    let m = height_cm / 100.0;
    Some(weight_kg / (m * m))
}

/// One-time migration of the legacy device-global localStorage profile (sex +
/// height) into the synced `profile` row.
///
/// No-op when a synced row already exists — this guards the login path so a
/// device's leftover localStorage never clobbers a newer account profile.
/// Otherwise, if either legacy key is present, build a row stamped with the
/// real now (so a genuinely-newer remote profile wins on the next pull), persist
/// it, and remove the legacy localStorage keys.
pub async fn migrate_from_local_storage() {
    if db::get::<ProfileRow>("profile", PROFILE_KEY).await.is_some() {
        return;
    }
    let Some(ls) = local_storage() else { return };
    let sex = ls.get_item(KEY_SEX).ok().flatten().filter(|v| v == "male" || v == "female");
    let height_cm = ls
        .get_item(KEY_HEIGHT)
        .ok()
        .flatten()
        .and_then(|v| v.parse::<f64>().ok())
        .filter(|h| *h > 0.0);

    if sex.is_none() && height_cm.is_none() {
        return;
    }

    let row = ProfileRow {
        key: PROFILE_KEY.to_string(),
        sex,
        height_cm,
        birth_year: None,
        goal: None,
        updated_at: chrono::Utc::now().to_rfc3339(),
    };
    db::put("profile", &row).await;
    let _ = ls.remove_item(KEY_SEX);
    let _ = ls.remove_item(KEY_HEIGHT);
}

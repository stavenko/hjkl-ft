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
    Gain,
    Maintain,
}

thread_local! {
    static CACHE: RefCell<Option<ProfileRow>> = const { RefCell::new(None) };
    /// Bumped whenever the cached row CHANGES (a sync brought another device's
    /// profile), so UI reading the synchronous getters re-renders.
    static VERSION: RefCell<Option<leptos::RwSignal<u32>>> = const { RefCell::new(None) };
}

/// Create the root reactivity signal. Call once from `main()` BEFORE the first
/// [`hydrate`] (which runs inside `db::init`).
pub fn init() {
    VERSION.with(|c| *c.borrow_mut() = Some(leptos::create_rw_signal(0u32)));
}

/// The signal UI subscribes to for profile changes (the getters below are
/// synchronous cache reads and are NOT reactive on their own).
pub fn version_signal() -> leptos::RwSignal<u32> {
    VERSION.with(|c| c.borrow().expect("profile::init() must run first"))
}

fn local_storage() -> Option<web_sys::Storage> {
    web_sys::window().and_then(|w| w.local_storage().ok().flatten())
}

/// Load the profile row from IndexedDB into the synchronous in-memory cache.
/// Called after the active database is switched (launch / login / pairing).
pub async fn hydrate() {
    let row = db::get::<ProfileRow>("profile", PROFILE_KEY).await;
    let changed = CACHE.with(|c| {
        let mut cur = c.borrow_mut();
        if *cur == row {
            return false;
        }
        *cur = row;
        true
    });
    // A profile arriving by SYNC must show at once: the persona screen and every
    // profile-derived widget read the cache synchronously, so without this bump
    // they keep the pre-sync state until the next launch.
    if changed {
        if let Some(sig) = VERSION.with(|c| *c.borrow()) {
            leptos::SignalUpdate::update(&sig, |v| *v += 1);
        }
    }
}

/// Read the cached row (a clone), or a fresh empty row keyed "profile".
fn row_or_default() -> ProfileRow {
    CACHE.with(|c| c.borrow().clone()).unwrap_or(ProfileRow {
        key: PROFILE_KEY.to_string(),
        sex: None,
        height_cm: None,
        birth_year: None,
        goal: None,
        cycle_start: None,
        steps_planka: None,
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

/// The daily steps planka (activity target), if set. System-set on the activity
/// week — there is no manual editing UI. Lives here (not in `goals`) so no
/// nutrient-iterating food UI can ever pick it up.
pub fn get_steps_planka() -> Option<f64> {
    CACHE.with(|c| c.borrow().as_ref().and_then(|r| r.steps_planka).filter(|p| *p > 0.0))
}

/// Store the steps planka. A non-positive value clears it.
pub fn set_steps_planka(planka: f64) {
    write(|r| r.steps_planka = if planka > 0.0 { Some(planka) } else { None });
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
            Some("gain") => CourseGoal::Gain,
            _ => CourseGoal::Lose,
        }
    })
}

/// Store the course goal. A CHANGE flags the calorie planka as needing a recompute
/// (the old planka was derived for the previous goal).
pub fn set_goal(goal: CourseGoal) {
    let changed = get_goal() != goal;
    let v = match goal {
        CourseGoal::Lose => "lose",
        CourseGoal::Gain => "gain",
        CourseGoal::Maintain => "maintain",
    };
    write(|r| r.goal = Some(v.to_string()));
    if changed {
        crate::services::local::set_planka_stale(true);
    }
}

/// First day of the current menstrual cycle (YYYY-MM-DD), if set.
pub fn get_cycle_start() -> Option<String> {
    CACHE.with(|c| c.borrow().as_ref().and_then(|r| r.cycle_start.clone()))
}

/// Store the first day of the cycle (YYYY-MM-DD). An empty string clears it.
pub fn set_cycle_start(date: &str) {
    let v = if date.is_empty() { None } else { Some(date.to_string()) };
    write(|r| r.cycle_start = v);
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

/// Current age in whole years from the stored birth year (approximate — no
/// birthday tracking). `None` if the birth year is unset/out of range.
pub fn get_age_years() -> Option<i32> {
    let current_year = chrono::Utc::now().format("%Y").to_string().parse::<i32>().unwrap_or(2026);
    get_birth_year().map(|by| current_year - by)
}

/// Daily protein target in grams, computed from estimated FAT-FREE MASS rather
/// than raw body weight. We estimate body-fat % with the Deurenberg (1991)
/// equation from BMI + age + sex — i.e. assuming an ordinary, UNTRAINED body
/// composition typical for the person's population — then take 1.6 g of protein
/// per kg of fat-free mass:
///
/// ```text
/// BF%   = 1.2·BMI + 0.23·age − 10.8·sex − 5.4      (sex: 1 male, 0 female)
/// FFM   = weight · (1 − BF%/100)
/// target = round(1.6 · FFM)
/// ```
///
/// Returns `None` if weight/height is non-positive (BMI undefined). The estimated
/// BF% is clamped to a physiological [3, 60]% before FFM so an out-of-range
/// extrapolation can't yield an absurd (or negative) target.
pub fn protein_target_g(weight_kg: f64, height_cm: f64, age_years: i32, sex: Sex) -> Option<u32> {
    if weight_kg <= 0.0 {
        return None;
    }
    let bmi = bmi(weight_kg, height_cm)?;
    let sex_term = match sex {
        Sex::Male => 1.0,
        Sex::Female => 0.0,
    };
    let bf_pct = (1.2 * bmi + 0.23 * age_years as f64 - 10.8 * sex_term - 5.4).clamp(3.0, 60.0);
    let ffm = weight_kg * (1.0 - bf_pct / 100.0);
    Some((1.6 * ffm).round() as u32)
}

/// Convenience over [`protein_target_g`]: pulls height/age/sex from the profile
/// and computes the target for `weight_kg`. Returns 0 when any of those is unset
/// (the setup section captures them before protein ever matters), so the task
/// simply shows «0 г» until the profile is complete — the same fallback the
/// weight-only formula used.
pub fn protein_target_from_profile(weight_kg: f64) -> u32 {
    match (get_height_cm(), get_age_years(), get_sex()) {
        (Some(h), Some(age), Some(sex)) => protein_target_g(weight_kg, h, age, sex).unwrap_or(0),
        _ => 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deurenberg_worked_example_man() {
        // Man, 35 y, 180 cm, 90 kg → BMI 27.8, BF% ≈ 25%, FFM ≈ 67.3 kg,
        // protein 1.6 g/kg FFM ≈ 108 g (the reference example).
        let g = protein_target_g(90.0, 180.0, 35, Sex::Male).unwrap();
        assert_eq!(g, 108);
    }

    #[test]
    fn deurenberg_woman_higher_bf_lower_target() {
        // Same anthropometrics but female (sex term 0) → higher BF%, less FFM,
        // so a lower protein target than the man.
        let woman = protein_target_g(90.0, 180.0, 35, Sex::Female).unwrap();
        let man = protein_target_g(90.0, 180.0, 35, Sex::Male).unwrap();
        assert!(woman < man, "woman {woman} should be < man {man}");
    }

    #[test]
    fn protein_target_needs_positive_weight_and_height() {
        assert!(protein_target_g(0.0, 180.0, 35, Sex::Male).is_none());
        assert!(protein_target_g(90.0, 0.0, 35, Sex::Male).is_none());
    }
}

//! Background per-food category classifier.
//!
//! As soon as a food is logged into the diary, its id is enqueued here; a single
//! app-scoped worker drains the queue ONE food at a time (`ai::classify_food` →
//! `local::cache_food_tags`), so foods are classified in order without a parallel
//! burst. Tags (`is_snack` / `is_liquid_cal` / `is_veg_fruit`) are cached on the
//! `Food` and synced. Idempotent: a food already fully tagged is skipped, so
//! re-logging a known food costs nothing.
//!
//! This replaces the old "tag snacks at daily-summary time" flow: classification
//! is now immediate and per-food, not deferred to the (now removed) daily report.

use std::cell::RefCell;
use std::collections::VecDeque;

use api_types::Food;
use leptos::spawn_local;

use super::{ai, db, local};

struct Queue {
    pending: VecDeque<String>,
    running: bool,
}

thread_local! {
    static Q: RefCell<Queue> = const { RefCell::new(Queue { pending: VecDeque::new(), running: false }) };
}

/// Reset the queue. Called once from `main()` for symmetry with other services
/// (the thread_local is already initialised lazily).
pub fn init() {
    Q.with(|q| {
        let mut q = q.borrow_mut();
        q.pending.clear();
        q.running = false;
    });
}

/// True if `food` still has an unassigned category tag.
fn needs_classification(food: &Food) -> bool {
    food.is_snack.is_none() || food.is_liquid_cal.is_none() || food.is_veg_fruit.is_none()
        || food.is_egg.is_none() || food.is_red_meat.is_none()
}

/// Enqueue a food id for background classification (no-op if already queued) and
/// start the worker if idle. Safe to call from anywhere (no UI context needed).
pub fn enqueue(food_id: String) {
    let start = Q.with(|q| {
        let mut q = q.borrow_mut();
        if q.pending.iter().any(|id| *id == food_id) {
            return false;
        }
        q.pending.push_back(food_id);
        if q.running {
            false
        } else {
            q.running = true;
            true
        }
    });
    if start {
        spawn_local(run_worker());
    }
}

/// Drain the queue one food at a time. A per-food error is logged (not silently
/// swallowed) and the queue moves on, so one bad food can never wedge the rest.
async fn run_worker() {
    loop {
        let next = Q.with(|q| q.borrow_mut().pending.pop_front());
        let Some(id) = next else {
            Q.with(|q| q.borrow_mut().running = false);
            return;
        };
        // Load the food; skip if gone or already fully tagged (e.g. a re-logged
        // known food, or one another device already classified).
        let Some(food) = db::get::<Food>("foods", &id).await else { continue };
        if !needs_classification(&food) {
            continue;
        }
        match ai::classify_food(&[food.name.clone()]).await {
            Ok(tags) if tags.len() == 1 => {
                local::cache_food_tags(&[(id, tags[0])]).await;
            }
            Ok(other) => {
                leptos::logging::warn!(
                    "classify: expected 1 verdict for {}, got {}", food.name, other.len()
                );
            }
            Err(e) => leptos::logging::warn!("classify failed for {}: {e}", food.name),
        }
    }
}

/// Enqueue every not-yet-classified food logged today or yesterday. Called on app
/// activation (launch + foreground) so anything logged offline, before this
/// feature existed, or on another device eventually gets tagged.
pub async fn sweep_diary_unclassified() {
    let today = chrono::Local::now().date_naive();
    let foods: std::collections::BTreeMap<String, Food> =
        local::list_foods().await.into_iter().map(|f| (f.id.clone(), f)).collect();
    let mut seen = std::collections::HashSet::new();
    for i in 0..2 {
        let d = (today - chrono::Duration::days(i)).format("%Y-%m-%d").to_string();
        for e in local::list_diary(&d).await {
            if let Some(food) = foods.get(&e.food_id) {
                if needs_classification(food) && seen.insert(food.id.clone()) {
                    enqueue(food.id.clone());
                }
            }
        }
    }
}

/// Enqueue every not-yet-classified RECIPE INGREDIENT food, so a dish's egg /
/// red-meat / veg-fruit content can be counted by composition. Called on activation
/// (covers recipes built before the ingredients were classified).
pub async fn sweep_recipe_ingredients() {
    let foods: std::collections::BTreeMap<String, Food> =
        local::list_foods().await.into_iter().map(|f| (f.id.clone(), f)).collect();
    let mut seen = std::collections::HashSet::new();
    for ing in local::list_recipe_ingredients().await {
        if let Some(food) = foods.get(&ing.food_id) {
            if needs_classification(food) && seen.insert(food.id.clone()) {
                enqueue(food.id.clone());
            }
        }
    }
}

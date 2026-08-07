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

use super::{ai, db, errors, local};

/// Online per the global connectivity probe (AI worker reachable) — the real
/// signal for background classification, which itself calls the AI worker.
fn is_online() -> bool {
    super::net::online_now()
}

/// Block until the browser reports connectivity again (polls; capped ~30 min).
async fn wait_for_online() {
    for _ in 0..600 {
        if is_online() {
            return;
        }
        ai::sleep_ms(3000).await;
    }
}

/// Run one background AI op with up to 3 attempts. On failure:
/// - if the device is OFFLINE → wait for connectivity, then try 3 more times
///   (a lost connection must never burn a food's error slot);
/// - if ONLINE but still failing (bad JSON / model error / transient) → record the
///   error and give up (the next activation sweep retries it anyway).
/// Returns `Some(value)` on success, `None` if given up.
async fn with_retries<F, Fut, T>(mut op: F, aspect: errors::FoodAspect, food: &str) -> Option<T>
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = Result<T, String>>,
{
    loop {
        let mut last = String::from("unknown error");
        for attempt in 0..3 {
            match op().await {
                Ok(v) => return Some(v),
                Err(e) => {
                    last = e;
                    if attempt < 2 {
                        ai::sleep_ms(1500).await;
                    }
                }
            }
        }
        if !is_online() {
            wait_for_online().await;
            continue; // connection is a temporary problem — retry the whole batch
        }
        errors::record_food(aspect, food, &last);
        return None;
    }
}

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

/// Drain the queue one food at a time (classify + nutrient enrichment). Every
/// failure is recorded in the error log so it stays visible.
async fn run_worker() {
    loop {
        let next = Q.with(|q| q.borrow_mut().pending.pop_front());
        let Some(id) = next else {
            Q.with(|q| q.borrow_mut().running = false);
            return;
        };
        // Load the food; skip if gone. One sequential background pass fills BOTH the
        // category tags and the extra nutrients (calcium/iron/omega/fiber) — two AI
        // calls at most, so the model isn't hit all at once.
        let Some(food) = db::get::<Food>("foods", &id).await else { continue };

        if needs_classification(&food) {
            let name = food.name.clone();
            if let Some(tags) = with_retries(
                move || { let n = name.clone(); async move { ai::classify_food(&[n]).await } },
                errors::FoodAspect::Kind,
                &food.name,
            ).await
            {
                if let Some(t) = tags.into_iter().next() {
                    local::cache_food_tags(&[(id.clone(), t)]).await;
                }
            }
        }

        // `food.nutrients` is unchanged by classification, so the loaded copy is
        // still current for the enrichment gate.
        if super::enrich::needs_enrichment(&food) {
            let f = food.clone();
            with_retries(
                move || { let f = f.clone(); async move { super::enrich::enrich_food(&f).await } },
                errors::FoodAspect::Nutrients,
                &food.name,
            ).await;
        }

        // Iron is its OWN pass: it needs the absorbed fraction alongside the
        // amount, so it can't ride the generic nutrient request (see `iron.rs`).
        // Runs only once the iron week is open — before that nothing reads it.
        if super::iron::unlocked() && super::iron::needs_iron(&food) {
            let f = food.clone();
            with_retries(
                move || { let f = f.clone(); async move { super::iron::enrich_iron(&f).await } },
                errors::FoodAspect::Iron,
                &food.name,
            ).await;
        }
    }
}

/// True if a food still needs any background AI processing (tags, nutrients, iron).
fn needs_processing(food: &Food) -> bool {
    needs_classification(food)
        || super::enrich::needs_enrichment(food)
        || (super::iron::unlocked() && super::iron::needs_iron(food))
}

/// Enqueue EVERY product in the database that still lacks something — tags,
/// nutrients or iron. Called on app activation (launch + foreground).
///
/// This used to walk the last two weeks of the DIARY instead. That window was cut
/// for the daily indicators, whose verdict spans seven days; it is far too short for
/// a weekly one — the iron indicator judges eight completed weeks, so six of them
/// could never be filled and stayed empty forever. And when a NEW nutrient is added
/// later, every product ever created is missing it, not just the recently eaten
/// ones. So the sweep is over the whole `foods` store, and the answer to «is there
/// anything to do» comes from the product itself.
///
/// It cannot loop: a product is enqueued only while something is genuinely absent,
/// and every pass either fills a field or records an error. A value of ZERO counts
/// as filled — «this food has no iron» is an answer, not a gap.
pub async fn sweep_unprocessed() {
    for food in local::list_foods().await {
        if needs_processing(&food) {
            enqueue(food.id.clone());
        }
    }
}

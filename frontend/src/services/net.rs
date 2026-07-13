//! Global connectivity state — "can we actually reach our servers".
//!
//! `navigator.onLine` only reports the network interface, which is useless in
//! Russia where the NIC is up but our workers are blocked unless a VPN is on. So
//! we PROBE the real backends: the AI worker (the critical one) drives the global
//! [`is_online`] flag, and a handful of secondary workers feed a [`degraded`]
//! list surfaced under the dashboard warning triangle.
//!
//! A probe is a `no-cors` GET to `{base}/health` raced against a timeout: it
//! RESOLVES for any reachable server (even an opaque/404 response) and REJECTS on
//! a real network failure — exactly the reachability signal we want, with no CORS
//! dependency on the health route.
//!
//! Signals are created at the ROOT via [`init`] (like `update::init`) so `set`
//! from a background task always hits a live handle.

use std::cell::RefCell;

use leptos::*;
use wasm_bindgen_futures::JsFuture;
use web_sys::{Request, RequestInit, RequestMode};

use super::config;

/// The backend workers we probe. `Ai` drives [`is_online`]; the rest feed the
/// [`degraded`] list (data still saves locally; those features are just paused).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Worker {
    Ai,
    Sync,
    Auth,
    Payment,
    Ocr,
    Bug,
    Support,
    Push,
}

impl Worker {
    fn base(self) -> &'static str {
        let c = config::get();
        match self {
            Worker::Ai => &c.ai_base_url,
            Worker::Sync => &c.sync_base_url,
            Worker::Auth => &c.auth_base_url,
            Worker::Payment => &c.payment_base_url,
            Worker::Ocr => &c.ocr_queue_base_url,
            Worker::Bug => &c.bug_report_base_url,
            Worker::Support => &c.support_base_url,
            Worker::Push => &c.push_base_url,
        }
    }

    /// i18n key for the user-facing worker name (shown in the degraded list).
    pub fn label_key(self) -> &'static str {
        match self {
            Worker::Ai => "net.worker.ai",
            Worker::Sync => "net.worker.sync",
            Worker::Auth => "net.worker.auth",
            Worker::Payment => "net.worker.payment",
            Worker::Ocr => "net.worker.ocr",
            Worker::Bug => "net.worker.bug",
            Worker::Support => "net.worker.support",
            Worker::Push => "net.worker.push",
        }
    }
}

thread_local! {
    // Tri-state: `None` = not probed yet (we genuinely don't know — draw no
    // warning AND treat as not-online, so nothing goes to the network on a guess);
    // `Some(true)` = AI worker reachable; `Some(false)` = unreachable. Starting
    // `None` (not optimistic `true`) means the offline warning shows the moment we
    // KNOW we're offline, and network actions wait for a confirmed `Some(true)`.
    static ONLINE: RefCell<Option<RwSignal<Option<bool>>>> = const { RefCell::new(None) };
    static DEGRADED: RefCell<Option<RwSignal<Vec<Worker>>>> = const { RefCell::new(None) };
}

/// Create the shared signals in the root reactive scope. Call once from main()
/// before mounting, alongside `db::init` / `update::init`.
pub fn init() {
    ONLINE.with(|c| {
        if c.borrow().is_none() {
            *c.borrow_mut() = Some(create_rw_signal(None));
        }
    });
    DEGRADED.with(|c| {
        if c.borrow().is_none() {
            *c.borrow_mut() = Some(create_rw_signal(Vec::new()));
        }
    });
}

/// Reactive tri-state: `None` until the first probe, then `Some(reachable)` for
/// the AI worker (our critical backend). Draw the offline warning only on
/// `Some(false)`; go to the network only on `Some(true)`.
pub fn is_online() -> RwSignal<Option<bool>> {
    ONLINE.with(|c| c.borrow().expect("net::init() must run before is_online()"))
}

/// Reactive list of secondary workers currently unreachable while online.
pub fn degraded() -> RwSignal<Vec<Worker>> {
    DEGRADED.with(|c| c.borrow().expect("net::init() must run before degraded()"))
}

/// Non-reactive "may I go to the network right now" — true ONLY when the AI
/// worker is confirmed reachable (`Some(true)`). `None` (unprobed) and
/// `Some(false)` both return false, so nothing acts on a guess.
pub fn online_now() -> bool {
    matches!(is_online().get_untracked(), Some(true))
}

/// Reachability of one worker: a `no-cors` GET to `{base}/health` raced against a
/// timeout. Empty base (config not fetched yet) → not reachable.
async fn reachable(base: &str) -> bool {
    if base.is_empty() {
        return false;
    }
    let url = format!("{base}/health");
    let opts = RequestInit::new();
    opts.set_method("GET");
    opts.set_mode(RequestMode::Cors);
    let Ok(request) = Request::new_with_str_and_init(&url, &opts) else {
        return false;
    };
    let Some(window) = web_sys::window() else {
        return false;
    };
    // Reachability = the fetch RESOLVES (any HTTP status — even 401/404 means the
    // server answered). Only a real network/CORS failure rejects → unreachable.
    // The `/health` route therefore serves wildcard CORS so this succeeds from any
    // origin; secondary workers answer (4xx) which still counts as reachable.
    matches!(
        with_timeout(4000, JsFuture::from(window.fetch_with_request(&request))).await,
        Some(Ok(_))
    )
}

/// Probe ONLY the AI worker (→ [`is_online`]). We deliberately don't proactively
/// probe the secondary workers — that was 8 `/health` requests per tick, a large
/// share of the account's daily request budget for a single user. The `degraded`
/// list is instead populated by REAL request failures (see [`note_failure`]) and
/// reset on reconnect. Fire-and-forget via [`probe_background`] from the bootstrap,
/// resume, connectivity events, and the periodic timer.
pub async fn probe() {
    let was = is_online().get_untracked();
    let ai_ok = reachable(Worker::Ai.base()).await;
    is_online().set(Some(ai_ok));
    if ai_ok && was != Some(true) {
        // (Re)connected — drop stale degraded entries; real failures re-add them.
        degraded().set(Vec::new());
    }
}

/// Fire-and-forget connectivity probe.
pub fn probe_background() {
    leptos::spawn_local(probe());
}

/// Immediately mark a worker down after a real request failure, without waiting
/// for the next scheduled probe. AI failure drops the global online flag; a
/// secondary failure adds it to the degraded list. A follow-up [`probe`] confirms
/// or clears it.
pub fn note_failure(worker: Worker) {
    match worker {
        Worker::Ai => is_online().set(Some(false)),
        w => degraded().update(|d| {
            if !d.contains(&w) {
                d.push(w);
            }
        }),
    }
}

async fn with_timeout<F: std::future::Future>(ms: u32, future: F) -> Option<F::Output> {
    use futures::future::Either;
    futures::pin_mut!(future);
    let sleep = sleep_ms(ms);
    futures::pin_mut!(sleep);
    match futures::future::select(future, sleep).await {
        Either::Left((output, _)) => Some(output),
        Either::Right((_, _)) => None,
    }
}

async fn sleep_ms(ms: u32) {
    let promise = js_sys::Promise::new(&mut |resolve, _| {
        web_sys::window()
            .unwrap()
            .set_timeout_with_callback_and_timeout_and_arguments_0(&resolve, ms as i32)
            .unwrap();
    });
    let _ = wasm_bindgen_futures::JsFuture::from(promise).await;
}

#[cfg(test)]
mod tests {
    use super::*;

    const ALL: &[Worker] = &[
        Worker::Ai,
        Worker::Sync,
        Worker::Auth,
        Worker::Payment,
        Worker::Ocr,
        Worker::Bug,
        Worker::Support,
        Worker::Push,
    ];

    #[test]
    fn label_keys_are_unique() {
        let mut keys: Vec<&str> = ALL.iter().map(|w| w.label_key()).collect();
        let total = keys.len();
        keys.sort_unstable();
        keys.dedup();
        assert_eq!(keys.len(), total, "duplicate net.worker label key");
    }

    #[test]
    fn every_worker_labels_under_net_namespace() {
        for w in ALL {
            assert!(w.label_key().starts_with("net.worker."));
        }
    }
}

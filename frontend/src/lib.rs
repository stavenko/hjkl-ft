pub mod app;
pub mod components;
pub mod pages;
pub mod services;

#[cfg(not(test))]
use wasm_bindgen::prelude::wasm_bindgen;

/// Poll interval for the update check (ms). 30s for now; will be raised to ~1h.
#[cfg(not(test))]
const UPDATE_POLL_MS: i32 = 30_000;
/// Interval for the connectivity re-probe (ms). RU networks flap, so we re-check
/// server reachability regularly to keep the offline warning honest.
#[cfg(not(test))]
const PROBE_POLL_MS: i32 = 15_000;

#[cfg(not(test))]
#[wasm_bindgen(start)]
pub fn main() {
    console_error_panic_hook::set_once();
    leptos::spawn_local(async {
        // ---- Critical path (under the splash): LOCAL ONLY, no network. <1s. ----
        // Config synchronously from cache-or-default so `config::get()` never
        // panics and network code has a base to read (empty until the background
        // fetch REPLACES it). The UI must not wait for the network or the config.
        services::config::load_or_default();

        services::db::init().await;
        services::app_flags::reload().await;
        // Switch to the signed-in user's per-user database before any sync. The
        // bootstrap (`hjkl-ft`) database belongs to this user — they were the last
        // signed-in account on the device — so migrate it in (one-time, rescues
        // local-only stores like progress photos), then keep using the per-user DB.
        if let Some(uid) = services::auth::get_user_id() {
            // This runs UNDER the splash (part of "teal-izing the DB"). The one-time
            // bootstrap→per-user migration copies stores, which can grow on a large
            // history — measure it, so a slow migration surfaces instead of hiding
            // behind a blank splash. If it regularly exceeds this, move its progress
            // into the splash rather than the critical path.
            let t0 = js_sys::Date::now();
            services::db::activate_for_user(&uid, true).await;
            let dt = js_sys::Date::now() - t0;
            if dt > 500.0 {
                leptos::logging::warn!("db::activate_for_user took {dt:.0}ms under the splash");
            }
            services::app_flags::activate().await;
        }
        services::i18n::init_lang();
        services::i18n::init_weight_unit();
        services::update::init(); // update-available signal at the root
        services::net::init(); // connectivity signals at the root
        services::subscription::init(); // subscription gate signal at the root
        services::story::init_attention(); // story-attention signal at the root
        services::classify::init(); // reset the background food-classification queue
        services::errors::init(); // background-error log signal at the root

        // The database is ready → drop the splash and show the UI IMMEDIATELY.
        // Everything below is background and MUST NOT block the first paint.
        if let Some(splash) = web_sys::window()
            .and_then(|w| w.document())
            .and_then(|d| d.get_element_by_id("splash"))
        {
            splash.remove();
        }

        leptos::mount_to_body(app::App);

        services::story::refresh_attention();

        // Background listeners/timers: focus re-sync, notification receipts,
        // connectivity re-probe on online/offline + a periodic probe, and the
        // update poll.
        install_foreground_sync();
        install_notif_receipt_poll();
        install_connectivity_listeners();
        install_periodic_probe();
        install_update_poll();

        // ---- Background network bootstrap: prepare the connection, then use it. ----
        leptos::spawn_local(bootstrap_network());
    });
}

/// Prepare the network in the background and, once the server is reachable, start
/// the network interactions. Never blocks the UI.
#[cfg(not(test))]
async fn bootstrap_network() {
    // 1. Fetch the real config from the network and swap it in live (+cache for
    //    next launch). Until this lands the base URLs may be the cached/empty set.
    if let Some(cfg) = services::config::fetch_from_network().await {
        services::config::save_to_cache(&cfg);
        services::config::set(cfg);
    }

    // 2. Probe reachability: AI worker → is_online; secondary workers → degraded.
    services::net::probe().await;

    // 3. Server reachable → begin network interactions.
    if services::net::online_now() {
        // First update check right after establishing the connection.
        services::update::check().await;
        // Subscription: first-ever verify or the daily re-check (flips the gate).
        services::subscription::maybe_recheck().await;
        // Reconcile with the server when signed in: push then pull the merged dump.
        if services::auth::get_token().is_some() {
            if let Err(e) = services::sync::sync_now().await {
                leptos::logging::warn!("Launch sync failed: {e}");
            }
        }
        // Classify any recent/untagged food now that the AI worker is reachable.
        leptos::spawn_local(services::classify::sweep_diary_unclassified());
        leptos::spawn_local(services::classify::sweep_recipe_ingredients());
    }
}

#[cfg(not(test))]
fn install_foreground_sync() {
    use wasm_bindgen::prelude::Closure;
    use wasm_bindgen::JsCast;

    let Some(document) = web_sys::window().and_then(|w| w.document()) else { return };
    let cb = Closure::<dyn FnMut()>::new(move || {
        // `document.hidden` is false when the tab/PWA is in the foreground.
        let hidden = web_sys::window()
            .and_then(|w| w.document())
            .and_then(|d| js_sys::Reflect::get(d.as_ref(), &"hidden".into()).ok())
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        if !hidden {
            // Re-probe connectivity FIRST: a resumed device may have lost/regained
            // the network (VPN toggled, tunnel dropped) — refresh is_online so the
            // warning is honest and the follow-ups gate correctly.
            services::net::probe_background();
            services::update::check_background();
            services::story::refresh_attention();
            if services::auth::get_token().is_some() {
                services::sync::sync_now_background();
            }
            // Daily subscription re-check (no-op unless a day has passed).
            leptos::spawn_local(services::subscription::maybe_recheck());
            // Classify any still-untagged recent food on resume too.
            leptos::spawn_local(services::classify::sweep_diary_unclassified());
        }
    });
    let _ = document
        .add_event_listener_with_callback("visibilitychange", cb.as_ref().unchecked_ref());
    // Leak the closure: it must live for the whole app session.
    cb.forget();
}

#[cfg(not(test))]
fn install_notif_receipt_poll() {
    use wasm_bindgen::prelude::Closure;
    use wasm_bindgen::JsCast;
    // A "task complete" notification (its URL carries `ntf=<kind>.<section>.<task>.<rand>`)
    // means that task's milestone is confirmed on receipt. The service worker records the
    // code in Cache; index.html bridges it into localStorage `rn_notif_received`. Poll it,
    // resolve the task's story flag and set it HERE in Leptos — set_flag bumps the story
    // db-version signal, so the checkmark updates reactively. No tap / navigation.
    let cb = Closure::<dyn Fn()>::new(move || {
        let Some(storage) = web_sys::window().and_then(|w| w.local_storage().ok().flatten())
        else {
            return;
        };
        let Some(code) = storage.get_item("rn_notif_received").ok().flatten() else { return };
        if code.is_empty() {
            return;
        }
        let _ = storage.remove_item("rn_notif_received");
        services::diag::note(&format!("WASM poll consumed {code}"));
        // code = "<kind>.<section>.<task>.<rand>" — the task id is the 3rd segment.
        let task = code.split('.').nth(2).unwrap_or_default().to_string();
        if let Some(flag) = services::story::flag_for_task(&task) {
            leptos::spawn_local(async move {
                services::story::set_flag(flag, true).await;
            });
        }
    });
    if let Some(win) = web_sys::window() {
        let _ = win.set_interval_with_callback_and_timeout_and_arguments_0(
            cb.as_ref().unchecked_ref(),
            1000,
        );
    }
    cb.forget();
}

/// Re-probe server reachability on the browser's `online`/`offline` events. These
/// events are only a TRIGGER (they report the NIC, not our servers) — the probe
/// is the source of truth.
#[cfg(not(test))]
fn install_connectivity_listeners() {
    use wasm_bindgen::prelude::Closure;
    use wasm_bindgen::JsCast;

    let Some(window) = web_sys::window() else { return };
    let cb = Closure::<dyn FnMut()>::new(move || {
        services::net::probe_background();
    });
    let _ = window.add_event_listener_with_callback("online", cb.as_ref().unchecked_ref());
    let _ = window.add_event_listener_with_callback("offline", cb.as_ref().unchecked_ref());
    cb.forget();
}

/// Periodically re-probe reachability so a silently dropped connection (common on
/// RU mobile networks) surfaces the warning without needing a resume.
#[cfg(not(test))]
fn install_periodic_probe() {
    use wasm_bindgen::prelude::Closure;
    use wasm_bindgen::JsCast;

    let Some(window) = web_sys::window() else { return };
    let cb = Closure::<dyn Fn()>::new(move || {
        services::net::probe_background();
    });
    let _ = window.set_interval_with_callback_and_timeout_and_arguments_0(
        cb.as_ref().unchecked_ref(),
        PROBE_POLL_MS,
    );
    cb.forget();
}

/// Poll `/version.json` on a timer so a freshly deployed build is offered without
/// a resume. `check_background` no-ops while offline.
#[cfg(not(test))]
fn install_update_poll() {
    use wasm_bindgen::prelude::Closure;
    use wasm_bindgen::JsCast;

    let Some(window) = web_sys::window() else { return };
    let cb = Closure::<dyn Fn()>::new(move || {
        services::update::check_background();
    });
    let _ = window.set_interval_with_callback_and_timeout_and_arguments_0(
        cb.as_ref().unchecked_ref(),
        UPDATE_POLL_MS,
    );
    cb.forget();
}

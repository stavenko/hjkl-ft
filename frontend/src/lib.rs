pub mod app;
pub mod components;
pub mod pages;
pub mod services;

#[cfg(not(test))]
use wasm_bindgen::prelude::wasm_bindgen;

#[cfg(not(test))]
#[wasm_bindgen(start)]
pub fn main() {
    console_error_panic_hook::set_once();
    leptos::spawn_local(async {
        let (online, _) = futures::join!(
            async {
                match with_timeout(3000, services::config::fetch_from_network()).await {
                    Some(Some(cfg)) => {
                        services::config::save_to_cache(&cfg);
                        services::config::set(cfg);
                        true
                    }
                    _ => {
                        services::config::load_from_cache();
                        false
                    }
                }
            },
            sleep_ms(800),
        );

        services::db::init().await;
        services::app_flags::reload().await;
        // Switch to the signed-in user's per-user database before any sync. The
        // bootstrap (`hjkl-ft`) database belongs to this user — they were the last
        // signed-in account on the device — so migrate it in (one-time, rescues
        // local-only stores like progress photos), then keep using the per-user DB.
        if let Some(uid) = services::auth::get_user_id() {
            services::db::activate_for_user(&uid, true).await;
            services::app_flags::activate().await;
        }
        services::i18n::init_lang();
        services::i18n::init_weight_unit();
        services::update::init(); // create the update-available signal at the root
        services::story::init_attention(); // create the story-attention signal at the root
        services::classify::init(); // reset the background food-classification queue

        // Reconcile with the server on launch when signed in: push local changes,
        // then pull the merged result (so changes — incl. deletions — made on other
        // devices arrive). A signed-out / offline launch simply skips sync.
        if online && services::auth::get_token().is_some() {
            if let Err(e) = services::sync::sync_now().await {
                leptos::logging::warn!("Launch sync failed: {e}");
            }
        }

        if let Some(splash) = web_sys::window()
            .and_then(|w| w.document())
            .and_then(|d| d.get_element_by_id("splash"))
        {
            splash.remove();
        }

        leptos::mount_to_body(app::App);

        // Re-reconcile with the server when the app regains focus, so a device
        // that was backgrounded picks up changes made on other devices; and
        // reload if a newer build was deployed (covers iOS PWA resumed from
        // memory, which never re-navigates).
        install_foreground_sync();
        install_notif_receipt_poll();
        services::update::check_background();
        services::story::refresh_attention();

        // On activation, classify any food logged today/yesterday that isn't tagged
        // yet (offline entries, other devices, pre-feature foods). Runs one food at
        // a time in the background.
        leptos::spawn_local(services::classify::sweep_diary_unclassified());
    });
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
            // Reload first if a new build shipped; otherwise reconcile with the server.
            services::update::check_background();
            services::story::refresh_attention();
            if services::auth::get_token().is_some() {
                services::sync::sync_now_background();
            }
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

#[cfg(not(test))]
async fn with_timeout<F: std::future::Future>(ms: u32, future: F) -> Option<F::Output> {
    use futures::future::{Either, FutureExt};
    futures::pin_mut!(future);
    let sleep = sleep_ms(ms);
    futures::pin_mut!(sleep);
    match futures::future::select(future, sleep).await {
        Either::Left((output, _)) => Some(output),
        Either::Right((_, _)) => None,
    }
}

#[cfg(not(test))]
async fn sleep_ms(ms: u32) {
    let promise = js_sys::Promise::new(&mut |resolve, _| {
        web_sys::window()
            .unwrap()
            .set_timeout_with_callback_and_timeout_and_arguments_0(&resolve, ms as i32)
            .unwrap();
    });
    let _ = wasm_bindgen_futures::JsFuture::from(promise).await;
}

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
        services::i18n::init_lang();
        services::i18n::init_weight_unit();

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
        services::update::check_background();

        // If the day rolled over since last open, pre-generate yesterday's
        // summary in the background (best-effort; gated by subscription).
        leptos::spawn_local(services::summary::pregen_on_day_change());
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
            if services::auth::get_token().is_some() {
                services::sync::sync_now_background();
            }
        }
    });
    let _ = document
        .add_event_listener_with_callback("visibilitychange", cb.as_ref().unchecked_ref());
    // Leak the closure: it must live for the whole app session.
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

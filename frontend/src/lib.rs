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

        if online && services::sync::is_empty().await {
            let _ = services::sync::pull_full_dump().await;
        }

        if let Some(splash) = web_sys::window()
            .and_then(|w| w.document())
            .and_then(|d| d.get_element_by_id("splash"))
        {
            splash.remove();
        }

        leptos::mount_to_body(app::App);
    });
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

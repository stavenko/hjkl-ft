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
        services::config::load().await;
        services::db::init().await;
        if services::sync::is_empty().await {
            if let Err(e) = services::sync::pull_full_dump().await {
                leptos::logging::warn!("Initial sync failed: {e}");
            }
        }
        leptos::mount_to_body(app::App);
    });
}

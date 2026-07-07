pub mod api;
pub mod app;
pub mod auth;
pub mod config;
pub mod datashare;

use wasm_bindgen::prelude::wasm_bindgen;

#[wasm_bindgen(start)]
pub fn main() {
    console_error_panic_hook::set_once();
    leptos::spawn_local(async {
        match config::fetch_from_network().await {
            Some(cfg) => {
                config::save_to_cache(&cfg);
                config::set(cfg);
            }
            None => config::load_from_cache(),
        }
        leptos::mount_to_body(|| leptos::view! { <app::App /> });
    });
}

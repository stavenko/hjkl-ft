pub mod api;
pub mod app;
pub mod auth;
pub mod config;

/// Разбор и отрисовка присланных наборов данных переехали в общий крейт: то же
/// самое читает кураторское приложение, и расходиться эти два чтения не должны.
/// Реэкспорт оставлен, чтобы `crate::datashare::…` в app.rs остался прежним.
pub use ::datashare;

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

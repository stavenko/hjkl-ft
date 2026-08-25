//! Кабинет куратора. Отдельное приложение на своём домене — со своим паскеем,
//! своим списком клиентов и своей перепиской.
//!
//! Данных худеющих здесь нет и не появляется: всё, что видит куратор, человек
//! прислал сам, отчётом. Приложение только показывает присланное и отправляет
//! обратно директивы — числа, из которых текст собирается уже у худеющего.

pub mod api;
pub mod app;
pub mod auth;
pub mod config;
pub mod i18n;

use wasm_bindgen::prelude::wasm_bindgen;

#[wasm_bindgen(start)]
pub fn main() {
    console_error_panic_hook::set_once();
    i18n::init();
    leptos::spawn_local(async {
        // Адреса воркеров нужны раньше первого запроса. Сеть может не ответить —
        // тогда берём прошлый ответ из кэша, как это делает админка.
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

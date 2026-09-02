//! Приложение тренировок re:Norma — gym.renorma.app.
//!
//! Отдельный проект на своём поддомене, но НЕ отдельный аккаунт: входят сюда тем
//! же паскеем, что и в fit.renorma.app. Это возможно потому, что в проде область
//! ключей у обоих одна — rp_id `renorma.app`, registrable suffix и для
//! `fit.renorma.app`, и для `gym.renorma.app` (см. `GYM_RP_ID` в
//! `cloudflare/auth-worker/wrangler.toml`). Тот же токен, тот же `sub`, та же
//! подписка.
//!
//! Первая версия — ОНБОРДИНГ и только он: войти ключом, проверить подписку,
//! поставить приложение на домашний экран (инструкция под конкретный браузер) и
//! показать заглушку. Тренировок здесь ещё нет; место под них подготовлено —
//! `cloudflare/gym-sync-worker` уже ждёт своих данных.

pub mod ai;
pub mod app;
pub mod auth;
pub mod config;
pub mod i18n;
pub mod install;
pub mod platform;
pub mod settings;
pub mod subscription;
pub mod update;

use wasm_bindgen::prelude::wasm_bindgen;

#[wasm_bindgen(start)]
pub fn main() {
    console_error_panic_hook::set_once();
    i18n::init();
    leptos::spawn_local(async {
        // Флаг «есть обновление» заводится в КОРНЕВОЙ области — до монтирования и
        // никогда лениво внутри перерисовываемого узла (см. update::init).
        update::init();

        // Адреса воркеров нужны раньше первого запроса, а сеть может не ответить
        // — тогда берём прошлый ответ из кэша. Так же, как в кураторском
        // приложении и в админке.
        match config::fetch_from_network().await {
            Some(cfg) => {
                config::save_to_cache(&cfg);
                config::set(cfg);
            }
            None => config::load_from_cache(),
        }

        // Конфигурация есть — снимаем экран загрузки и показываем интерфейс.
        // Всё, что ниже, идёт фоном и первую отрисовку не задерживает.
        if let Some(splash) = web_sys::window()
            .and_then(|w| w.document())
            .and_then(|d| d.get_element_by_id("splash"))
        {
            splash.remove();
        }
        // Приложение поднялось — снимаем сторож зависшего обновления. Место
        // единственно верное: ровно здесь мы впервые знаем, что запуск удался.
        update::note_app_started();

        leptos::mount_to_body(|| leptos::view! { <app::App /> });

        // Проверка обновления — один раз при запуске и потом при каждом
        // возвращении на передний план. Фонового опроса нет и не надо: сборка
        // выкатывается руками, а установленное приложение живёт неделями, и
        // именно возвращение к нему — тот момент, когда стоит спросить.
        update::check_background();
        install_resume_check();
    });
}

/// Перепроверять обновление, когда приложение возвращается на передний план.
///
/// `visibilitychange`, а не `focus`: установленное приложение сворачивают, а не
/// расфокусируют, и на телефоне это единственное надёжное событие «человек снова
/// смотрит сюда».
fn install_resume_check() {
    use wasm_bindgen::prelude::Closure;
    use wasm_bindgen::JsCast;

    let Some(document) = web_sys::window().and_then(|w| w.document()) else { return };
    let cb = Closure::<dyn FnMut()>::new(move || {
        let hidden = web_sys::window()
            .and_then(|w| w.document())
            .map(|d| d.hidden())
            .unwrap_or(false);
        if !hidden {
            update::check_background();
        }
    });
    let _ = document
        .add_event_listener_with_callback("visibilitychange", cb.as_ref().unchecked_ref());
    // Слушатель живёт столько же, сколько страница: снимать его некому и незачем.
    cb.forget();
}

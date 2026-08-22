use std::cell::Cell;

use serde::{Deserialize, Serialize};
use wasm_bindgen::JsCast;
use wasm_bindgen_futures::JsFuture;
use web_sys::{Request, RequestInit, RequestMode, Response};

const CONFIG_URL: &str = "/config/frontend.toml";
const LS_KEY: &str = "ft_config_cache";

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct FrontendConfig {
    #[serde(default)]
    pub api_base_url: String,
    #[serde(default)]
    pub auth_base_url: String,
    #[serde(default)]
    pub push_base_url: String,
    #[serde(default)]
    pub ai_base_url: String,
    #[serde(default)]
    pub payment_base_url: String,
    #[serde(default)]
    pub ocr_queue_base_url: String,
    #[serde(default)]
    pub sync_base_url: String,
    #[serde(default)]
    pub bug_report_base_url: String,
    #[serde(default)]
    pub support_base_url: String,
    /// Public marketing/subscription site. "Регистрация" on the login screen sends a
    /// new user here to subscribe (registration itself happens in the paid claim flow).
    #[serde(default)]
    pub landing_url: String,
    /// Telegram deep-link to the payment bot's Mini App (the «оформить подписку» entry).
    #[serde(default)]
    pub miniapp_pay_url: String,
    /// This app's own origin (used as a fallback when `window.location.origin` is
    /// unavailable). Differs per env (renorma-fit-dev.pages.dev vs fit.renorma.app).
    #[serde(default)]
    pub app_origin: String,
    /// Модель стороннего провайдера для КАРТИНОК (этикетка, фото блюда). Уходит в
    /// наш ai-worker, который по имени модели маршрутизирует запрос наружу. Пусто —
    /// картинки идут прежним путём: очередь ocr-queue и свой сервер с Qwen2.5-VL.
    #[serde(default)]
    pub vision_model: String,
    /// Модель ОПОЗНАНИЯ еды — первого узла конвейера признаков. Пусто — опознание
    /// идёт на Workers AI, как и остальные узлы.
    ///
    /// Отдельная настройка, потому что опознание решает судьбу всего прохода: не
    /// узнали продукт — признаки не спрашиваются вовсе. Замер показал, что здесь
    /// модель покрупнее окупается (Кабачок, Редиска, Сердце цыплят-бройлеров
    /// проходят 4/4 против 0–1/3), а на самих признаках разницы такой нет — там
    /// работа по справочникам, и они остаются на дешёвой модели.
    #[serde(default)]
    pub identity_model: String,

    /// ЗАПАСНАЯ модель опознания — на случай, когда основная не ответила.
    ///
    /// Основная живёт у стороннего провайдера, и отказать она может по причинам, к
    /// продукту отношения не имеющим: кончилась квота, не прошёл платёж, лежит сам
    /// провайдер. Повторять у него же бессмысленно, поэтому вторая попытка уходит
    /// сюда — обычно к той же модели на Workers AI (`@cf/qwen/qwen3.8-27b`).
    ///
    /// Пусто — запасной нет, повтор идёт к основной, как было раньше.
    #[serde(default)]
    pub identity_fallback_model: String,
}

thread_local! {
    // The live config. WASM is single-threaded, so a thread_local Cell holding a
    // leaked `&'static` gives us a `get() -> &'static FrontendConfig` that never
    // moves (callers keep borrowing it) yet can be REPLACED at runtime: the
    // background network fetch swaps in the fresh config without a reload. `set`
    // leaks the previous value — a handful of tiny leaks over a session, which is
    // fine for a config struct and buys an unchanged, allocation-free `get()`.
    static CURRENT: Cell<Option<&'static FrontendConfig>> = const { Cell::new(None) };
}

pub fn get() -> &'static FrontendConfig {
    CURRENT
        .with(|c| c.get())
        .expect("Frontend config not initialized")
}

/// True once a config (cache/default/network) has been installed.
pub fn is_initialized() -> bool {
    CURRENT.with(|c| c.get().is_some())
}

pub fn set(cfg: FrontendConfig) {
    let leaked: &'static FrontendConfig = Box::leak(Box::new(cfg));
    CURRENT.with(|c| c.set(Some(leaked)));
}

/// Install a config synchronously WITHOUT touching the network: the cached one if
/// present, else `Default`. This is the offline-first bootstrap — it lets the UI
/// mount immediately; the network fetch later REPLACES this via [`set`].
pub fn load_or_default() {
    if !is_initialized() {
        set(read_ls().unwrap_or_default());
    }
}

/// Гарантировать, что адреса воркеров известны. На ПЕРВОМ открытии в новом
/// браузере кэша конфига нет, и `load_or_default` ставит пустой — тогда любой
/// запрос ушёл бы относительным путём на наш же Pages-домен и вернул 405,
/// а приложение прочитало бы это как «ссылка устарела». Поэтому здесь мы ЖДЁМ
/// сеть; провал — громкая ошибка, а не молчаливый пропуск.
pub async fn ensure_ready() -> Result<(), String> {
    if !get().auth_base_url.is_empty() {
        return Ok(());
    }
    match fetch_from_network().await {
        Some(cfg) => {
            save_to_cache(&cfg);
            set(cfg);
            if get().auth_base_url.is_empty() {
                let msg = "конфигурация загружена, но адрес auth-воркера пуст".to_string();
                leptos::logging::error!("config::ensure_ready: {msg}");
                return Err(msg);
            }
            Ok(())
        }
        None => {
            let msg = "не удалось загрузить конфигурацию приложения".to_string();
            leptos::logging::error!("config::ensure_ready: {msg}");
            Err(msg)
        }
    }
}

pub fn save_to_cache(cfg: &FrontendConfig) {
    let Ok(json) = serde_json::to_string(cfg) else { return };
    let Some(storage) = window_storage() else { return };
    let _ = storage.set_item(LS_KEY, &json);
}

pub async fn fetch_from_network() -> Option<FrontendConfig> {
    let opts = RequestInit::new();
    opts.set_method("GET");
    opts.set_mode(RequestMode::Cors);

    let request = Request::new_with_str_and_init(CONFIG_URL, &opts).ok()?;
    let window = web_sys::window()?;
    let resp = JsFuture::from(window.fetch_with_request(&request)).await.ok()?;
    let response: Response = resp.dyn_into().ok()?;

    if !response.ok() {
        return None;
    }

    let text_promise = response.text().ok()?;
    let text_value = JsFuture::from(text_promise).await.ok()?;
    let text = text_value.as_string()?;

    toml::from_str::<FrontendConfig>(&text).ok()
}

fn read_ls() -> Option<FrontendConfig> {
    let storage = window_storage()?;
    let json = storage.get_item(LS_KEY).ok()??;
    serde_json::from_str(&json).ok()
}

fn window_storage() -> Option<web_sys::Storage> {
    web_sys::window()?.local_storage().ok()?
}

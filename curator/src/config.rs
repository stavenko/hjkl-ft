//! Конфигурация кураторского приложения: адреса воркеров и origin приложения
//! худеющего. Читается при запуске из `/config/frontend.toml`, кэшируется в
//! localStorage — как в админке, тем же способом и по той же причине: сеть может
//! не ответить, а адреса нужны сразу.

use std::sync::OnceLock;

use serde::{Deserialize, Serialize};
use wasm_bindgen::JsCast;
use wasm_bindgen_futures::JsFuture;
use web_sys::{Request, RequestInit, RequestMode, Response};

const CONFIG_URL: &str = "/config/frontend.toml";
const LS_KEY: &str = "curator_config_cache";

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CuratorConfig {
    #[serde(default)]
    pub auth_base_url: String,
    #[serde(default)]
    pub support_base_url: String,
    #[serde(default)]
    pub push_base_url: String,
    /// Origin приложения ХУДЕЮЩЕГО — из него строится пригласительная ссылка,
    /// которую куратор копирует и отправляет человеку.
    #[serde(default)]
    pub app_origin: String,
}

static CONFIG: OnceLock<CuratorConfig> = OnceLock::new();

pub fn get() -> &'static CuratorConfig {
    CONFIG.get().expect("curator config not initialized")
}

pub fn set(cfg: CuratorConfig) {
    let _ = CONFIG.set(cfg);
}

pub fn load_from_cache() {
    let cfg = read_ls().unwrap_or_default();
    let _ = CONFIG.set(cfg);
}

pub fn save_to_cache(cfg: &CuratorConfig) {
    let Ok(json) = serde_json::to_string(cfg) else { return };
    let Some(storage) = window_storage() else { return };
    let _ = storage.set_item(LS_KEY, &json);
}

pub async fn fetch_from_network() -> Option<CuratorConfig> {
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
    let text_value = JsFuture::from(response.text().ok()?).await.ok()?;
    let text = text_value.as_string()?;
    toml::from_str::<CuratorConfig>(&text).ok()
}

fn read_ls() -> Option<CuratorConfig> {
    let storage = window_storage()?;
    let json = storage.get_item(LS_KEY).ok()??;
    serde_json::from_str(&json).ok()
}

fn window_storage() -> Option<web_sys::Storage> {
    web_sys::window()?.local_storage().ok()?
}

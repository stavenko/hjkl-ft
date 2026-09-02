//! Конфигурация приложения тренировок: адреса воркеров. Читается при запуске из
//! `/config/frontend.toml`, кэшируется в localStorage — как в кураторском
//! приложении, тем же способом и по той же причине: сеть может не ответить, а
//! адреса нужны сразу.

use std::sync::OnceLock;

use serde::{Deserialize, Serialize};
use wasm_bindgen::JsCast;
use wasm_bindgen_futures::JsFuture;
use web_sys::{Request, RequestInit, RequestMode, Response};

const CONFIG_URL: &str = "/config/frontend.toml";
const LS_KEY: &str = "gym_config_cache";

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct GymConfig {
    /// auth-worker: паскей, тот же, что у приложения худеющего.
    #[serde(default)]
    pub auth_base_url: String,
    /// payment-worker: подписка. Она общая с приложением питания — одна оплата,
    /// оба приложения.
    #[serde(default)]
    pub payment_base_url: String,
    /// ai-worker: генерация фразы восстановления (подписочный, тот же токен).
    /// Больше приложению тренировок от модели пока ничего не нужно.
    #[serde(default)]
    pub ai_base_url: String,
    /// Свой журнал синхронизации. Первая версия в него ещё не пишет; адрес здесь
    /// заведён вместе с воркером, чтобы не заводить его врозь.
    #[serde(default)]
    pub gym_sync_base_url: String,
}

static CONFIG: OnceLock<GymConfig> = OnceLock::new();

pub fn get() -> &'static GymConfig {
    CONFIG.get().expect("gym config not initialized")
}

pub fn set(cfg: GymConfig) {
    let _ = CONFIG.set(cfg);
}

pub fn load_from_cache() {
    let cfg = read_ls().unwrap_or_default();
    let _ = CONFIG.set(cfg);
}

pub fn save_to_cache(cfg: &GymConfig) {
    let Ok(json) = serde_json::to_string(cfg) else { return };
    let Some(storage) = window_storage() else { return };
    let _ = storage.set_item(LS_KEY, &json);
}

pub async fn fetch_from_network() -> Option<GymConfig> {
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
    toml::from_str::<GymConfig>(&text).ok()
}

fn read_ls() -> Option<GymConfig> {
    let storage = window_storage()?;
    let json = storage.get_item(LS_KEY).ok()??;
    serde_json::from_str(&json).ok()
}

fn window_storage() -> Option<web_sys::Storage> {
    web_sys::window()?.local_storage().ok()?
}

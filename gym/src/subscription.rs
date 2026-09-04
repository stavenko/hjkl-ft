//! Подписка. Та же самая, что у приложения питания: один аккаунт — одна оплата,
//! оба приложения. Спрашиваем тот же `GET /subscription` у payment-worker тем же
//! токеном; своего понятия «подписка на зал» здесь нет и заводить его не надо.
//!
//! Кэш — как в приложении худеющего и по той же причине: вернувшийся человек
//! должен войти мгновенно, не дожидаясь сети. Живая проверка идёт следом и, если
//! статус изменился, экран сам переключится.

use serde::{Deserialize, Serialize};
use wasm_bindgen::JsCast;
use wasm_bindgen::JsValue;
use wasm_bindgen_futures::JsFuture;

use crate::{auth, config};

const LS_KEY: &str = "gym_subscription";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Status {
    #[serde(default)]
    pub plan: String,
    #[serde(default)]
    pub end: i64,
    pub active: bool,
}

/// Последний известный статус. Пусто — статус не спрашивали ни разу.
pub fn cached() -> Option<Status> {
    let json = storage()?.get_item(LS_KEY).ok()??;
    serde_json::from_str(&json).ok()
}

fn cache(status: &Status) {
    let (Ok(json), Some(s)) = (serde_json::to_string(status), storage()) else { return };
    let _ = s.set_item(LS_KEY, &json);
}

/// Забыть статус. Зовётся при выходе: следующий вход — другой аккаунт, и чужой
/// ответ «подписка активна» пустил бы его внутрь без проверки.
pub fn forget() {
    if let Some(s) = storage() {
        let _ = s.remove_item(LS_KEY);
    }
}

/// Почему статус не удалось узнать.
///
/// Разделение принципиальное. «Токен не приняли» и «до сервера не достучались» —
/// разные беды с разным лечением, и свалить их в одну строку значит показать
/// человеку с протухшей сессией экран «нет связи», по которому он будет чинить
/// вайфай вместо того, чтобы войти заново.
pub enum StatusError {
    /// 401: сессия протухла или аккаунт больше не существует. Надо входить заново.
    Unauthorized,
    /// Всё остальное: сети нет, воркер отдал 5xx, тело не разобралось.
    Unavailable(String),
}

impl std::fmt::Display for StatusError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            StatusError::Unauthorized => write!(f, "401 unauthorized"),
            StatusError::Unavailable(e) => write!(f, "{e}"),
        }
    }
}

/// Спросить живой статус у payment-worker и запомнить его.
pub async fn status() -> Result<Status, StatusError> {
    let unavailable = |e: String| StatusError::Unavailable(e);
    // Токена нет — входить заново, а не чинить сеть.
    let token = auth::get_token().ok_or(StatusError::Unauthorized)?;
    let base = &config::get().payment_base_url;
    if base.is_empty() {
        // Иначе запрос ушёл бы относительным путём на наш собственный домен и
        // вернул 405, который вызывающий принял бы за отказ сервера.
        return Err(unavailable("payment_base_url не сконфигурирован".to_string()));
    }
    let url = format!("{base}/subscription");

    let js = |e: JsValue| unavailable(format!("{e:?}"));

    let opts = web_sys::RequestInit::new();
    opts.set_method("GET");
    let headers = web_sys::Headers::new().map_err(js)?;
    headers
        .set("Authorization", &format!("Bearer {token}"))
        .map_err(js)?;
    opts.set_headers(&headers);

    let request = web_sys::Request::new_with_str_and_init(&url, &opts).map_err(js)?;
    let window = web_sys::window().expect("no window");
    let resp_val = JsFuture::from(window.fetch_with_request(&request)).await.map_err(js)?;
    let resp: web_sys::Response = resp_val
        .dyn_into()
        .map_err(|_| unavailable("not a Response".to_string()))?;

    let text = JsFuture::from(resp.text().map_err(js)?).await.map_err(js)?;
    let text = text
        .as_string()
        .ok_or_else(|| unavailable("response not string".to_string()))?;

    if resp.status() == 401 {
        return Err(StatusError::Unauthorized);
    }
    if !resp.ok() {
        return Err(unavailable(format!("HTTP {}: {}", resp.status(), text)));
    }
    let s: Status =
        serde_json::from_str(&text).map_err(|e| unavailable(format!("parse error: {e}")))?;
    cache(&s);
    Ok(s)
}

/// Состояние аккаунта, известное БЕЗ входа — по одному user_id.
///
/// Установленное приложение знает свой user_id (он в `start_url` манифеста), но
/// войти по ключу может не получиться. Это единственный способ различить
/// «оплатил, но не может войти» и «здесь платить ещё не начинали»: первому
/// предлагаем вход по коду, второго отправляем туда, где оформляют подписку.
/// Ручка неавторизованная — токена-то как раз и нет.
#[derive(Clone, Copy, Debug, Deserialize)]
pub struct AccountState {
    /// Тот же признак доступа, по которому пускают остальные воркеры.
    pub active: bool,
    /// Доходил ли человек до работающего приложения.
    #[serde(default)]
    pub entered: bool,
}

pub async fn account_state(user_id: &str) -> Result<AccountState, String> {
    let base = &config::get().payment_base_url;
    if base.is_empty() {
        return Err("payment_base_url не сконфигурирован".to_string());
    }
    let url = format!("{base}/account/state");
    let body = serde_json::json!({ "userId": user_id }).to_string();

    let opts = web_sys::RequestInit::new();
    opts.set_method("POST");
    opts.set_body(&JsValue::from_str(&body));
    let headers = web_sys::Headers::new().map_err(|e| format!("{e:?}"))?;
    headers.set("Content-Type", "application/json").map_err(|e| format!("{e:?}"))?;
    opts.set_headers(&headers);

    let request = web_sys::Request::new_with_str_and_init(&url, &opts).map_err(|e| format!("{e:?}"))?;
    let window = web_sys::window().expect("no window");
    let resp_val = JsFuture::from(window.fetch_with_request(&request))
        .await
        .map_err(|e: JsValue| format!("{e:?}"))?;
    let resp: web_sys::Response = resp_val.dyn_into().map_err(|_| "not a Response".to_string())?;
    let text = JsFuture::from(resp.text().map_err(|e| format!("{e:?}"))?)
        .await
        .map_err(|e| format!("{e:?}"))?;
    let text = text.as_string().ok_or("response not string")?;
    if !resp.ok() {
        return Err(format!("HTTP {}: {}", resp.status(), text));
    }
    serde_json::from_str(&text).map_err(|e| format!("parse error: {e}"))
}

fn storage() -> Option<web_sys::Storage> {
    web_sys::window()?.local_storage().ok()?
}

//! Клиент кураторского приложения к support-worker. Всё под токеном куратора.
//!
//! FAIL LOUDLY: каждый вызов возвращает `Result<_, ApiError>`; молчаливых
//! пустых ответов нет.

use serde::{Deserialize, Serialize};
use wasm_bindgen::{JsCast, JsValue};
use wasm_bindgen_futures::JsFuture;

use crate::{auth, config};

/// Отказ запроса. `Auth` — воркер отверг сам токен: 401 (протух или негоден) или
/// 403 (профиля куратора нет). Сессия мертва, и вызывающая сторона обязана выйти
/// на экран входа, а не опрашивать её вечно. `Other` — всё остальное: сеть,
/// ошибка сервера, разбор; показать надо, но сессию это не отменяет.
#[derive(Debug, Clone)]
pub enum ApiError {
    /// Token rejected (401/403). Carries the worker's message.
    Auth(String),
    /// Any other failure (network, 4xx/5xx, parse).
    Other(String),
}

impl ApiError {
    pub fn message(&self) -> &str {
        match self {
            ApiError::Auth(m) | ApiError::Other(m) => m,
        }
    }

    pub fn is_auth(&self) -> bool {
        matches!(self, ApiError::Auth(_))
    }
}

impl std::fmt::Display for ApiError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.message())
    }
}

/// One message in a thread.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub seq: u64,
    #[serde(default)]
    pub client_id: String,
    pub sender: String, // "user" | "expert"
    #[serde(default)]
    pub expert_id: Option<String>,
    pub text: String,
    pub created_at: String,
    /// "text" | "data_request" | "data_share". Old rows/messages with no kind
    /// deserialize as plain text (backward compatible).
    #[serde(default = "default_kind")]
    pub kind: String,
    /// Typed envelope for data_request / data_share. The worker stores and returns
    /// it as a RAW JSON STRING (not an embedded object) — parse it with
    /// `serde_json::from_str` at the use site. NULL/absent for plain text.
    #[serde(default)]
    pub payload: Option<String>,
    /// Имя, которым подписано сообщение куратора у худеющего.
    #[serde(default)]
    pub sender_name: Option<String>,
}

fn default_kind() -> String {
    "text".to_string()
}

#[derive(Debug, Deserialize)]
pub struct MessagesPage {
    pub messages: Vec<Message>,
    pub next_after_seq: u64,
    pub has_more: bool,
}

#[derive(Serialize)]
struct ReplyReq<'a> {
    client_id: &'a str,
    text: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    kind: Option<&'a str>,
    /// RAW JSON STRING — the worker reads `body.payload` as a string. Sending an
    /// object here makes the worker's `.as_str()` return None → payload dropped.
    #[serde(skip_serializing_if = "Option::is_none")]
    payload: Option<String>,
}

/// Профиль куратора.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct Curator {
    #[serde(default)]
    pub curator_id: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub lang: String,
}

/// Слот клиента. `invite_code` есть, только пока слот не привязан: после
/// согласия он погашен, и его место занимают данные человека.
#[derive(Debug, Clone, Deserialize)]
pub struct Client {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub invite_code: Option<String>,
    #[serde(default)]
    pub bound: bool,
    #[serde(default)]
    pub bound_at: Option<String>,
    #[serde(default)]
    pub last_report_at: Option<String>,
    #[serde(default)]
    pub request_days: Option<u32>,
    #[serde(default)]
    pub request_at: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ClientsResp {
    clients: Vec<Client>,
}

#[derive(Debug, Deserialize)]
struct ClientResp {
    client: Client,
}

#[derive(Debug, Deserialize)]
struct CuratorResp {
    #[serde(default)]
    found: bool,
    // `created` в ответе сервера есть (идемпотентность регистрации проверяется им
    // в scripts/curator-e2e.mjs), но приложению он ни к чему: и первый вызов, и
    // повторный дают один и тот же профиль.
    curator: Option<Curator>,
}

/// Последний отчёт клиента и состояние открытого запроса.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct ReportResp {
    /// Сырой JSON отчёта строкой — разбирается на месте показа.
    #[serde(default)]
    pub report: Option<String>,
    #[serde(default)]
    pub report_at: Option<String>,
    #[serde(default)]
    pub request_days: Option<u32>,
    #[serde(default)]
    pub request_at: Option<String>,
}

fn base() -> Result<String, ApiError> {
    let b = config::get().support_base_url.clone();
    if b.is_empty() {
        return Err(ApiError::Other("support_base_url is not configured".to_string()));
    }
    Ok(b)
}

async fn request<O: serde::de::DeserializeOwned>(
    method: &str,
    path: &str,
    body: Option<String>,
) -> Result<O, ApiError> {
    request_to(&base()?, method, path, body).await
}

async fn request_to<O: serde::de::DeserializeOwned>(
    base_url: &str,
    method: &str,
    path: &str,
    body: Option<String>,
) -> Result<O, ApiError> {
    let url = format!("{}{}", base_url, path);
    let token = auth::get_token().ok_or_else(|| ApiError::Auth("not authenticated".to_string()))?;

    let opts = web_sys::RequestInit::new();
    opts.set_method(method);
    if let Some(b) = &body {
        opts.set_body(&JsValue::from_str(b));
    }

    let headers = web_sys::Headers::new().map_err(|e| ApiError::Other(format!("{e:?}")))?;
    headers
        .set("Authorization", &format!("Bearer {token}"))
        .map_err(|e| ApiError::Other(format!("{e:?}")))?;
    if body.is_some() {
        headers
            .set("Content-Type", "application/json")
            .map_err(|e| ApiError::Other(format!("{e:?}")))?;
    }
    opts.set_headers(&headers);

    let request = web_sys::Request::new_with_str_and_init(&url, &opts)
        .map_err(|e| ApiError::Other(format!("{e:?}")))?;
    let window = web_sys::window().expect("no window");
    let resp_val = JsFuture::from(window.fetch_with_request(&request))
        .await
        .map_err(|e| ApiError::Other(format!("{e:?}")))?;
    let resp: web_sys::Response = resp_val
        .dyn_into()
        .map_err(|_| ApiError::Other("not a Response".to_string()))?;

    let text = JsFuture::from(resp.text().map_err(|e| ApiError::Other(format!("{e:?}")))?)
        .await
        .map_err(|e| ApiError::Other(format!("{e:?}")))?;
    let text = text
        .as_string()
        .ok_or_else(|| ApiError::Other("response not a string".to_string()))?;

    if !resp.ok() {
        let msg = format!("HTTP {}: {}", resp.status(), text);
        // 401 (протухший токен) и 403 (профиля куратора нет) — это смерть самой
        // сессии. Отличаем, чтобы уйти на вход, а не опрашивать её вечно.
        if resp.status() == 401 || resp.status() == 403 {
            return Err(ApiError::Auth(msg));
        }
        return Err(ApiError::Other(msg));
    }
    serde_json::from_str(&text).map_err(|e| ApiError::Other(format!("parse error: {e}")))
}

// ── Профиль ──────────────────────────────────────────────────────────────────

/// Завести профиль куратора под своим `sub`. Идемпотентно: повторный вызов
/// возвращает уже заведённый. Зовётся после КАЖДОГО входа, а не только после
/// регистрации: аппрува нет, и профиль — единственное, что отличает куратора от
/// любого другого владельца токена.
pub async fn register() -> Result<Curator, ApiError> {
    let r: CuratorResp = request("POST", "/curator/register", Some("{}".to_string())).await?;
    r.curator.ok_or_else(|| ApiError::Other("register: нет профиля в ответе".to_string()))
}

pub async fn me() -> Result<Option<Curator>, ApiError> {
    let r: CuratorResp = request("GET", "/curator/me", None).await?;
    Ok(if r.found { r.curator } else { None })
}

pub async fn set_profile(name: &str, lang: &str) -> Result<Curator, ApiError> {
    let body = serde_json::json!({ "name": name, "lang": lang }).to_string();
    let r: CuratorResp = request("POST", "/curator/me", Some(body)).await?;
    r.curator.ok_or_else(|| ApiError::Other("профиль не вернулся".to_string()))
}

// ── Клиенты ──────────────────────────────────────────────────────────────────

pub async fn clients() -> Result<Vec<Client>, ApiError> {
    let r: ClientsResp = request("GET", "/curator/clients", None).await?;
    Ok(r.clients)
}

pub async fn add_client(name: &str) -> Result<Client, ApiError> {
    let body = serde_json::json!({ "name": name }).to_string();
    let r: ClientResp = request("POST", "/curator/clients", Some(body)).await?;
    Ok(r.client)
}

pub async fn rename_client(id: &str, name: &str) -> Result<Client, ApiError> {
    let body = serde_json::json!({ "name": name }).to_string();
    let r: ClientResp =
        request("POST", &format!("/curator/clients/{id}/rename"), Some(body)).await?;
    Ok(r.client)
}

pub async fn delete_client(id: &str) -> Result<(), ApiError> {
    let _: serde_json::Value =
        request("POST", &format!("/curator/clients/{id}/delete"), Some("{}".to_string())).await?;
    Ok(())
}

/// Прекратить работу с человеком. Слот остаётся в списке — с новой ссылкой.
pub async fn unbind_client(id: &str) -> Result<(), ApiError> {
    let _: serde_json::Value =
        request("POST", &format!("/curator/clients/{id}/unbind"), Some("{}".to_string())).await?;
    Ok(())
}

// ── Данные ───────────────────────────────────────────────────────────────────

/// Попросить данные за `days` дней. Запрос уходит СООБЩЕНИЕМ в тред: приложение
/// худеющего и так читает его, и из него же считает состояние своего виджета.
pub async fn request_data(id: &str, days: u32) -> Result<u64, ApiError> {
    let body = serde_json::json!({
        "client_id": uuid::Uuid::now_v7().to_string(),
        "days": days,
    })
    .to_string();
    let v: serde_json::Value =
        request("POST", &format!("/curator/clients/{id}/request"), Some(body)).await?;
    v.get("seq")
        .and_then(|s| s.as_u64())
        .ok_or_else(|| ApiError::Other("запрос данных: нет seq".to_string()))
}

pub async fn report(id: &str) -> Result<ReportResp, ApiError> {
    request("GET", &format!("/curator/clients/{id}/report"), None).await
}

/// Поставить планку. Директива несёт ЧИСЛО и вид — текст худеющий соберёт у
/// себя, на своём языке.
pub async fn set_planka(id: &str, key: &str, amount: f64) -> Result<u64, ApiError> {
    let payload = serde_json::json!({ "key": key, "amount": amount });
    let body = serde_json::to_string(&ReplyReq {
        client_id: &uuid::Uuid::now_v7().to_string(),
        text: "",
        kind: Some("set_planka_v2"),
        payload: Some(payload.to_string()),
    })
    .map_err(|e| ApiError::Other(e.to_string()))?;
    let v: serde_json::Value =
        request("POST", &format!("/curator/clients/{id}/reply"), Some(body)).await?;
    v.get("seq")
        .and_then(|s| s.as_u64())
        .ok_or_else(|| ApiError::Other("правка планки: нет seq".to_string()))
}

// ── Переписка ────────────────────────────────────────────────────────────────

pub async fn messages(id: &str, after_seq: u64) -> Result<MessagesPage, ApiError> {
    request(
        "GET",
        &format!("/curator/clients/{id}/messages?after_seq={after_seq}&limit=200"),
        None,
    )
    .await
}

/// Длинный опрос: воркер держит запрос открытым до `wait` секунд.
pub async fn messages_wait(id: &str, after_seq: u64, wait: u32) -> Result<MessagesPage, ApiError> {
    request(
        "GET",
        &format!("/curator/clients/{id}/messages?after_seq={after_seq}&limit=200&wait={wait}"),
        None,
    )
    .await
}

pub async fn reply(id: &str, text: &str) -> Result<u64, ApiError> {
    let body = serde_json::to_string(&ReplyReq {
        client_id: &uuid::Uuid::now_v7().to_string(),
        text,
        kind: None,
        payload: None,
    })
    .map_err(|e| ApiError::Other(e.to_string()))?;
    let v: serde_json::Value =
        request("POST", &format!("/curator/clients/{id}/reply"), Some(body)).await?;
    v.get("seq")
        .and_then(|s| s.as_u64())
        .ok_or_else(|| ApiError::Other("ответ: нет seq".to_string()))
}

pub async fn mark_read(id: &str, seq: u64) -> Result<(), ApiError> {
    let body = serde_json::json!({ "seq": seq }).to_string();
    let _: serde_json::Value =
        request("POST", &format!("/curator/clients/{id}/read"), Some(body)).await?;
    Ok(())
}

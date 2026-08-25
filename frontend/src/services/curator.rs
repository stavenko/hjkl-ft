//! Связь худеющего с куратором: приглашение, согласие, отвязка.
//!
//! Транспорт — тот же, что у чата (support-worker, тот же токен), поэтому и
//! используется его: заводить второй ради трёх запросов незачем.
//!
//! FAIL LOUDLY: каждый путь возвращает `Result<_, String>`; вызывающая сторона
//! показывает ошибку человеку, а не делает вид, что ничего не случилось.

use serde::Deserialize;

use crate::services::support_chat::{get_json, post_json};

/// Что показать на экране согласия.
#[derive(Debug, Clone, Deserialize)]
pub struct InvitePeek {
    /// Приглашение существует и ещё не погашено.
    #[serde(default)]
    pub found: bool,
    /// Приглашением уже воспользовались.
    #[serde(default)]
    pub used: bool,
    #[serde(default)]
    pub curator_name: String,
    /// Человек уже у другого куратора — согласие оборвёт прежнюю связь, и
    /// сказать об этом надо ДО, а не после.
    #[serde(default)]
    pub current_curator_id: Option<String>,
}

/// Кто курирует этого человека сейчас.
#[derive(Debug, Clone, Deserialize)]
pub struct Binding {
    #[serde(default)]
    pub bound: bool,
    #[serde(default)]
    pub curator_name: String,
}

pub async fn peek(code: &str) -> Result<InvitePeek, String> {
    get_json(&format!("/curator/invite/{code}")).await
}

pub async fn accept(code: &str) -> Result<serde_json::Value, String> {
    post_json(&format!("/curator/invite/{code}/accept"), "{}").await
}

pub async fn binding() -> Result<Binding, String> {
    get_json("/curator/binding").await
}

/// Отвязаться от куратора по своей воле.
pub async fn unbind() -> Result<serde_json::Value, String> {
    post_json("/curator/unbind", "{}").await
}

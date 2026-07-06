//! Support-chat message storage. A single, flat chat (no chat list): every
//! user / assistant message is one record in the `chat` IndexedDB store, keyed
//! by a time-sortable uuid v7. The in-flight streaming bubble is transient UI
//! state in the page; only the FINAL assistant message is persisted here.

use serde::{Deserialize, Serialize};

use super::db;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    pub id: String,                 // uuid::Uuid::now_v7().to_string() — also the IndexedDB key
    pub role: String,               // "user" | "assistant" | "tool_call"
    pub text: String,               // user-typed text OR streamed assistant answer (final). May be "".
    pub image: Option<String>,      // image data URL "data:<mime>;base64,..." (user msgs only)
    pub audio: Option<String>,      // audio data URL "data:audio/webm;base64,..." (user msgs only)
    pub duration_secs: Option<f64>, // voice clip wall-clock seconds (user voice msgs only)
    pub escalated: bool,            // assistant msg flag set when escalate_to_human tool fired
    // tool_call messages: which tool the assistant invoked, its compact-JSON
    // params, and the compact-JSON result. `#[serde(default)]` so messages
    // stored before these fields existed still deserialize.
    #[serde(default)]
    pub tool_name: Option<String>,
    #[serde(default)]
    pub tool_params: Option<String>,
    #[serde(default)]
    pub tool_result: Option<String>,
    pub created_at: String,         // chrono::Utc::now().to_rfc3339() — ordering key (ascending)
}

fn now() -> String {
    chrono::Utc::now().to_rfc3339()
}

fn new_id() -> String {
    uuid::Uuid::now_v7().to_string()
}

/// All chat messages, oldest first (chat reads top → bottom). `list_all` is
/// unordered, so sort explicitly by `created_at`.
pub async fn list_messages() -> Vec<ChatMessage> {
    let mut msgs: Vec<ChatMessage> = db::list_all("chat").await;
    msgs.sort_by(|a, b| a.created_at.cmp(&b.created_at));
    msgs
}

/// Persist a user message (text + optional staged image / voice attachment).
pub async fn append_user(
    text: String,
    image: Option<String>,
    audio: Option<String>,
    duration_secs: Option<f64>,
) -> ChatMessage {
    let m = ChatMessage {
        id: new_id(),
        role: "user".to_string(),
        text,
        image,
        audio,
        duration_secs,
        escalated: false,
        tool_name: None,
        tool_params: None,
        tool_result: None,
        created_at: now(),
    };
    db::put("chat", &m).await;
    m
}

/// Persist the final assistant message once its stream has finished.
pub async fn append_assistant(text: String, escalated: bool) -> ChatMessage {
    let m = ChatMessage {
        id: new_id(),
        role: "assistant".to_string(),
        text,
        image: None,
        audio: None,
        duration_secs: None,
        escalated,
        tool_name: None,
        tool_params: None,
        tool_result: None,
        created_at: now(),
    };
    db::put("chat", &m).await;
    m
}

/// Persist a tool-call record (shown in the chat as "Assistant requested tool:
/// …" and gathered into the "Context" section with its result).
pub async fn append_tool_call(name: String, params: String, result: String) -> ChatMessage {
    let m = ChatMessage {
        id: new_id(),
        role: "tool_call".to_string(),
        text: String::new(),
        image: None,
        audio: None,
        duration_secs: None,
        escalated: false,
        tool_name: Some(name),
        tool_params: Some(params),
        tool_result: Some(result),
        created_at: now(),
    };
    db::put("chat", &m).await;
    m
}

pub async fn clear_chat() {
    db::clear("chat").await;
}

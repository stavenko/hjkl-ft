use serde::{Deserialize, Serialize};

/// JWT claims — identical shape to auth-worker so tokens minted there validate here.
#[derive(Debug, Serialize, Deserialize)]
pub struct TokenClaims {
    pub sub: String,
    pub iat: i64,
    pub exp: i64,
    pub caps: Vec<String>,
    #[serde(default)]
    pub token_id: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ErrorResponse {
    pub error: String,
}

// ---- wire: message ----
#[derive(Debug, Serialize, Deserialize)]
pub struct Message {
    pub seq: u64,
    pub client_id: String,
    pub sender: String, // "user" | "expert"
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expert_id: Option<String>,
    pub text: String,
    pub created_at: String, // RFC3339, DISPLAY ONLY
    // Typed data-request / data-share envelope. Old rows/messages default to
    // kind="text", payload=null.
    #[serde(default = "default_kind")]
    pub kind: String, // "text" | "data_request" | "data_share"
    // RAW stored JSON string, or null. Always emitted (never skipped) so the
    // read shape is stable; clients parse the string themselves.
    #[serde(default)]
    pub payload: Option<String>,
    /// Подпись отправителя-эксперта: имя куратора. У худеющего один экран чата на
    /// всех собеседников, и без подписи куратор неотличим от поддержки. Пусто у
    /// сообщений самого человека и у старых строк.
    #[serde(default)]
    pub sender_name: Option<String>,
}

fn default_kind() -> String {
    "text".to_string()
}

/// Append result returned by ConversationDO (internal).
#[derive(Debug, Serialize, Deserialize)]
pub struct AppendResult {
    pub seq: u64,
    pub created_at: String,
    pub deduped: bool,
}

/// GET messages response.
#[derive(Debug, Serialize, Deserialize)]
pub struct MessagesPage {
    pub messages: Vec<Message>,
    pub next_after_seq: u64,
    pub has_more: bool,
}

/// Conversation index row (expert list).
#[derive(Debug, Serialize, Deserialize)]
pub struct ConversationSummary {
    pub user_id: String,
    pub preview: String,
    pub last_ts: String,
    pub last_seq: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pending_since: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ConversationsPage {
    pub conversations: Vec<ConversationSummary>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_after: Option<String>,
    pub has_more: bool,
}

// ---- wire: куратор и его клиенты ----

/// Профиль куратора. `name` показывается худеющему на экране согласия и в чате,
/// `lang` — язык интерфейса самого куратора (на тексты, которые видит худеющий,
/// он не влияет: те собираются у худеющего на его языке).
#[derive(Debug, Serialize, Deserialize)]
pub struct CuratorProfile {
    pub curator_id: String,
    pub name: String,
    pub lang: String,
    pub created_at: String,
}

/// Слот клиента у куратора. `invite_code` присутствует ТОЛЬКО пока слот не
/// привязан: после согласия код погашен, и его место в интерфейсе занимают
/// данные человека.
#[derive(Debug, Serialize, Deserialize)]
pub struct ClientRow {
    pub id: String,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub invite_code: Option<String>,
    pub bound: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bound_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unbound_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_report_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub request_days: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub request_at: Option<String>,
}

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

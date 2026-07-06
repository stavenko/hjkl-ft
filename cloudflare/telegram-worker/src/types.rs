use serde::Deserialize;

/// Telegram Bot API Update — only the fields this bot needs. Everything optional so
/// unrelated update kinds deserialize fine and fall through to a 200 no-op.
#[derive(Debug, Deserialize)]
pub struct Update {
    #[serde(default)]
    pub message: Option<Message>,
}

#[derive(Debug, Deserialize)]
pub struct Message {
    pub chat: Chat,
    #[serde(default)]
    pub text: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct Chat {
    pub id: i64,
}

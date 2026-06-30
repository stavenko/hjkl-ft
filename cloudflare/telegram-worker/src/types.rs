use serde::Deserialize;

/// Telegram Bot API Update — only the fields this bot needs. Everything optional so
/// unrelated update kinds deserialize fine and fall through to a 200 no-op.
#[derive(Debug, Deserialize)]
pub struct Update {
    #[serde(default)]
    pub message: Option<Message>,
    #[serde(default)]
    pub callback_query: Option<CallbackQuery>,
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

#[derive(Debug, Deserialize)]
pub struct CallbackQuery {
    pub id: String,
    #[serde(default)]
    pub data: Option<String>,
    // Bears chat.id; message_id not needed for the fresh-sendMessage flow.
    #[serde(default)]
    pub message: Option<Message>,
}

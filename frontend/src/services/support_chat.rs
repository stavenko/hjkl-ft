//! Live support thread, backed by the support-worker (a SEPARATE server from the
//! AI `chat` store). The server is the source of truth; local IndexedDB holds a
//! message cache (keyed by server `seq`), the poll cursor, and an optimistic
//! outbox of in-flight / failed sends.
//!
//! FAIL LOUDLY: every transport path returns `Result<_, String>`; fire-and-forget
//! callers log on `Err` (never swallow). No sample data — the cache is only ever
//! populated from real server responses.

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use wasm_bindgen::{JsCast, JsValue};
use wasm_bindgen_futures::JsFuture;

use super::{auth, config, db};

/// The two threads the `/chat` toggle switches between. AI = the existing local
/// AI chat; Live = this server-backed support thread.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ChatMode {
    Ai,
    Live,
}

/// Cached server message, keyed by `seq` (IndexedDB key + ordering).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LiveMessage {
    pub seq: u64,
    /// Idempotency key the sender generated; present on server rows. Used to
    /// reconcile an optimistic outbox item once it returns as a server message.
    #[serde(default)]
    pub client_id: String,
    pub sender: String, // "user" | "expert" — MATCHES the worker's field name
    pub text: String,
    pub created_at: String,
    /// Message kind: "text" (plain), "data_request" (curator asks for a dataset),
    /// or "data_share" (user's shared dataset). Old rows (no field) → "text".
    #[serde(default = "default_kind")]
    pub kind: String,
    /// Typed envelope, a RAW JSON STRING (or null for plain text). Parsed by the
    /// bubble renderer per `kind`. Old rows (no field) → None.
    #[serde(default)]
    pub payload: Option<String>,
}

fn default_kind() -> String {
    "text".to_string()
}

/// Optimistic outbox entry, keyed by `client_id` (idempotency key + IndexedDB
/// key). Acked items are deleted from the outbox once they become server messages.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OutboxItem {
    pub client_id: String,
    pub text: String,
    pub status: String, // "sending" | "failed"
    pub created_at: String,
    /// Message kind for this in-flight send (so a retried data_share keeps its
    /// envelope). Old rows (no field) → "text".
    #[serde(default = "default_kind")]
    pub kind: String,
    /// Typed envelope (RAW JSON STRING) for a data_share send; None for text.
    #[serde(default)]
    pub payload: Option<String>,
}

/// Cursor singleton in the `support_meta` store, key "cursor".
#[derive(Debug, Clone, Serialize, Deserialize)]
struct Cursor {
    key: String,
    after_seq: u64,
}

#[derive(Serialize)]
struct SendReq<'a> {
    client_id: &'a str,
    text: &'a str,
    /// Omitted for a plain-text send (server defaults to "text"); set for a
    /// data_share message.
    #[serde(skip_serializing_if = "Option::is_none")]
    kind: Option<&'a str>,
    /// The typed envelope as a RAW JSON STRING; None for plain text.
    #[serde(skip_serializing_if = "Option::is_none")]
    payload: Option<&'a str>,
}

#[derive(Deserialize)]
struct SendAck {
    seq: u64,
    created_at: String,
}

#[derive(Deserialize)]
struct PollResp {
    messages: Vec<LiveMessage>,
    next_after_seq: u64,
    has_more: bool,
}

#[derive(Serialize)]
struct ReadReq {
    seq: u64,
}

fn now() -> String {
    chrono::Utc::now().to_rfc3339()
}

const MESSAGES_STORE: &str = "support_messages";
const OUTBOX_STORE: &str = "support_outbox";
const META_STORE: &str = "support_meta";
const CURSOR_KEY: &str = "cursor";

// ── Transport (FAIL LOUDLY, JWT-authed; mirrors sync.rs / bug_report.rs) ──

/// POST `body` (JSON) to `{support_base_url}{path}` and parse the JSON response.
async fn post_json<O: DeserializeOwned>(path: &str, body: &str) -> Result<O, String> {
    let base = &config::get().support_base_url;
    if base.is_empty() {
        return Err("support_base_url is not configured".to_string());
    }
    let url = format!("{base}{path}");
    let token = auth::get_token().ok_or_else(|| "not authenticated".to_string())?;

    let opts = web_sys::RequestInit::new();
    opts.set_method("POST");
    opts.set_body(&JsValue::from_str(body));

    let headers = web_sys::Headers::new().map_err(|e| format!("{e:?}"))?;
    headers.set("Content-Type", "application/json").map_err(|e| format!("{e:?}"))?;
    headers.set("Authorization", &format!("Bearer {token}")).map_err(|e| format!("{e:?}"))?;
    opts.set_headers(&headers);

    let request =
        web_sys::Request::new_with_str_and_init(&url, &opts).map_err(|e| format!("{e:?}"))?;
    let window = web_sys::window().expect("no window");
    let resp_val = JsFuture::from(window.fetch_with_request(&request))
        .await
        .map_err(|e| format!("{e:?}"))?;
    let resp: web_sys::Response = resp_val.dyn_into().map_err(|_| "not a Response".to_string())?;

    let text = JsFuture::from(resp.text().map_err(|e| format!("{e:?}"))?)
        .await
        .map_err(|e| format!("{e:?}"))?;
    let text = text.as_string().ok_or("response not a string")?;

    if !resp.ok() {
        return Err(format!("HTTP {}: {}", resp.status(), text));
    }
    serde_json::from_str(&text).map_err(|e| format!("parse error: {e}"))
}

/// GET `{support_base_url}{path}` (query in the URL) and parse the JSON response.
async fn get_json<O: DeserializeOwned>(path: &str) -> Result<O, String> {
    let base = &config::get().support_base_url;
    if base.is_empty() {
        return Err("support_base_url is not configured".to_string());
    }
    let url = format!("{base}{path}");
    let token = auth::get_token().ok_or_else(|| "not authenticated".to_string())?;

    let opts = web_sys::RequestInit::new();
    opts.set_method("GET");

    let headers = web_sys::Headers::new().map_err(|e| format!("{e:?}"))?;
    headers.set("Authorization", &format!("Bearer {token}")).map_err(|e| format!("{e:?}"))?;
    opts.set_headers(&headers);

    let request =
        web_sys::Request::new_with_str_and_init(&url, &opts).map_err(|e| format!("{e:?}"))?;
    let window = web_sys::window().expect("no window");
    let resp_val = JsFuture::from(window.fetch_with_request(&request))
        .await
        .map_err(|e| format!("{e:?}"))?;
    let resp: web_sys::Response = resp_val.dyn_into().map_err(|_| "not a Response".to_string())?;

    let text = JsFuture::from(resp.text().map_err(|e| format!("{e:?}"))?)
        .await
        .map_err(|e| format!("{e:?}"))?;
    let text = text.as_string().ok_or("response not a string")?;

    if !resp.ok() {
        return Err(format!("HTTP {}: {}", resp.status(), text));
    }
    serde_json::from_str(&text).map_err(|e| format!("parse error: {e}"))
}

// ── Public API ──

/// Send a new Live message. Writes an optimistic outbox item immediately, POSTs
/// (idempotent by `client_id`), and on ack reconciles into the message cache.
pub async fn send(text: String) -> Result<LiveMessage, String> {
    send_typed(text, "text".to_string(), None).await
}

/// Send a typed message — a data_share (kind="data_share", `payload` = the
/// envelope JSON string) or plain text. Same optimistic outbox + reconcile path
/// as [`send`]; the confirmation `text` is what shows optimistically.
pub async fn send_data_share(text: String, payload: String) -> Result<LiveMessage, String> {
    send_typed(text, "data_share".to_string(), Some(payload)).await
}

async fn send_typed(
    text: String,
    kind: String,
    payload: Option<String>,
) -> Result<LiveMessage, String> {
    let client_id = uuid::Uuid::now_v7().to_string();
    let item = OutboxItem {
        client_id: client_id.clone(),
        text: text.clone(),
        status: "sending".to_string(),
        created_at: now(),
        kind: kind.clone(),
        payload: payload.clone(),
    };
    db::put(OUTBOX_STORE, &item).await;
    post_with_outbox(client_id, text, item.created_at, kind, payload).await
}

/// Retry a failed outbox item: flip it back to "sending" and re-POST with the SAME
/// `client_id` (idempotent — not a new send path).
pub async fn retry(client_id: String) -> Result<LiveMessage, String> {
    let existing: Option<OutboxItem> = db::get(OUTBOX_STORE, &client_id).await;
    let Some(mut item) = existing else {
        return Err(format!("outbox item not found: {client_id}"));
    };
    item.status = "sending".to_string();
    db::put(OUTBOX_STORE, &item).await;
    post_with_outbox(client_id, item.text, item.created_at, item.kind, item.payload).await
}

/// Shared POST + reconcile path for `send` and `retry`. On success the acked
/// message lands in the cache and the outbox row is removed; on failure the outbox
/// row is marked "failed" (retryable) and the error is returned.
async fn post_with_outbox(
    client_id: String,
    text: String,
    created_at: String,
    kind: String,
    payload: Option<String>,
) -> Result<LiveMessage, String> {
    // Only send kind/payload when this is a typed (non-text) message.
    let (kind_field, payload_field) = if kind == "text" {
        (None, None)
    } else {
        (Some(kind.as_str()), payload.as_deref())
    };
    let body = serde_json::to_string(&SendReq {
        client_id: &client_id,
        text: &text,
        kind: kind_field,
        payload: payload_field,
    })
    .map_err(|e| e.to_string())?;

    match post_json::<SendAck>("/message", &body).await {
        Ok(ack) => {
            let msg = LiveMessage {
                seq: ack.seq,
                client_id: client_id.clone(),
                sender: "user".to_string(),
                text,
                created_at: ack.created_at,
                kind,
                payload,
            };
            db::put(MESSAGES_STORE, &msg).await;
            db::delete(OUTBOX_STORE, &client_id).await;
            // Advance the cursor past this seq so the next poll doesn't re-deliver it.
            let cursor = load_cursor().await;
            if ack.seq >= cursor {
                store_cursor(ack.seq).await;
            }
            Ok(msg)
        }
        Err(e) => {
            let failed = OutboxItem {
                client_id,
                text,
                status: "failed".to_string(),
                created_at,
                kind,
                payload,
            };
            db::put(OUTBOX_STORE, &failed).await;
            Err(e)
        }
    }
}

/// Poll the server from the stored cursor, paging until `has_more` is false. Each
/// message is upserted by `seq` (idempotent), and the cursor only advances forward.
pub async fn poll() -> Result<(), String> {
    loop {
        let after = load_cursor().await;
        let r: PollResp = get_json(&format!("/messages?after_seq={after}&limit=100")).await?;
        for m in &r.messages {
            db::put(MESSAGES_STORE, m).await;
            // Reconcile a lost-ack optimistic send: if this server message carries
            // a client_id we still have in the outbox, drop the outbox row (it's now
            // a real message) — prevents a permanent duplicate + stuck "sending".
            if !m.client_id.is_empty() {
                db::delete(OUTBOX_STORE, &m.client_id).await;
            }
        }
        store_cursor(r.next_after_seq).await;
        if !r.has_more {
            break;
        }
    }
    Ok(())
}

/// Advance the server-side read marker. Fire-and-forget at the call site.
pub async fn read(seq: u64) -> Result<(), String> {
    let body = serde_json::to_string(&ReadReq { seq }).map_err(|e| e.to_string())?;
    let _: serde_json::Value = post_json("/read", &body).await?;
    Ok(())
}

/// All cached Live messages, ordered by `seq` ascending.
pub async fn list_messages() -> Vec<LiveMessage> {
    let mut msgs: Vec<LiveMessage> = db::list_all(MESSAGES_STORE).await;
    msgs.sort_by_key(|m| m.seq);
    msgs
}

/// All outbox items (optimistic / failed), ordered by `created_at` ascending.
pub async fn list_outbox() -> Vec<OutboxItem> {
    let mut items: Vec<OutboxItem> = db::list_all(OUTBOX_STORE).await;
    items.sort_by(|a, b| a.created_at.cmp(&b.created_at));
    items
}

async fn load_cursor() -> u64 {
    let c: Option<Cursor> = db::get(META_STORE, CURSOR_KEY).await;
    c.map(|c| c.after_seq).unwrap_or(0)
}

async fn store_cursor(after_seq: u64) {
    // Only ever advance forward (a stale page must not rewind the cursor).
    let current = load_cursor().await;
    if after_seq < current {
        return;
    }
    db::put(META_STORE, &Cursor { key: CURSOR_KEY.to_string(), after_seq }).await;
}

// ── Persisted mode toggle (per-user-per-device, in app_flags; NOT synced) ──

const MODE_FLAG: &str = "support_chat_mode";

pub fn load_mode() -> ChatMode {
    match crate::services::app_flags::get(MODE_FLAG).as_deref() {
        Some("live") => ChatMode::Live,
        _ => ChatMode::Ai,
    }
}

pub fn save_mode(m: ChatMode) {
    crate::services::app_flags::set(MODE_FLAG, mode_str(m));
}

fn mode_str(m: ChatMode) -> &'static str {
    match m {
        ChatMode::Live => "live",
        ChatMode::Ai => "ai",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mode_str_round_trips() {
        assert_eq!(mode_str(ChatMode::Ai), "ai");
        assert_eq!(mode_str(ChatMode::Live), "live");
    }

    #[test]
    fn mode_default_is_ai() {
        // The string mapping `load_mode` relies on: anything that isn't "live"
        // (including a missing flag) is AI.
        assert!(matches!(
            match None::<&str> {
                Some("live") => ChatMode::Live,
                _ => ChatMode::Ai,
            },
            ChatMode::Ai
        ));
        assert!(matches!(
            match Some("ai") {
                Some("live") => ChatMode::Live,
                _ => ChatMode::Ai,
            },
            ChatMode::Ai
        ));
        assert!(matches!(
            match Some("live") {
                Some("live") => ChatMode::Live,
                _ => ChatMode::Ai,
            },
            ChatMode::Live
        ));
    }
}

//! Expert-side client for the support-worker. JWT-authed (the signed-in expert's
//! token). FAIL LOUDLY: every call returns `Result<_, String>`.

use serde::{Deserialize, Serialize};
use wasm_bindgen::{JsCast, JsValue};
use wasm_bindgen_futures::JsFuture;

use crate::{auth, config};

/// A request failure. `Auth` means the worker rejected the token itself (401
/// expired/invalid, or 403 sub not in EXPERT_IDS) — the session is dead and the
/// caller must log out and return to Login. `Other` is any transient/server
/// error that should be surfaced but does NOT invalidate the session.
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

/// A conversation row in the expert queue.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConversationSummary {
    pub user_id: String,
    pub preview: String,
    pub last_ts: String,
    pub last_seq: u64,
    /// When the oldest still-unanswered user message arrived. `Some` ⇒ waiting.
    #[serde(default)]
    pub pending_since: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct ConversationsPage {
    pub conversations: Vec<ConversationSummary>,
    #[serde(default)]
    pub next_after: Option<String>,
    pub has_more: bool,
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

fn base() -> Result<String, ApiError> {
    let b = config::get().support_base_url.clone();
    if b.is_empty() {
        return Err(ApiError::Other("support_base_url is not configured".to_string()));
    }
    Ok(b)
}

fn payment_base() -> Result<String, ApiError> {
    let b = config::get().payment_base_url.clone();
    if b.is_empty() {
        return Err(ApiError::Other("payment_base_url is not configured".to_string()));
    }
    Ok(b)
}

/// Same JWT-authed request as `request`, but against the support-worker base.
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
        // 401 (expired/invalid token) and 403 (sub not in EXPERT_IDS) mean the
        // session itself is dead — distinguish so the caller logs out and
        // returns to Login instead of polling a doomed session forever.
        if resp.status() == 401 || resp.status() == 403 {
            return Err(ApiError::Auth(msg));
        }
        return Err(ApiError::Other(msg));
    }
    serde_json::from_str(&text).map_err(|e| ApiError::Other(format!("parse error: {e}")))
}

/// Pending conversations, oldest-waiting first (the worker orders by `pending_seq`).
pub async fn list_pending(after: Option<&str>) -> Result<ConversationsPage, ApiError> {
    let mut path = "/conversations?status=pending&limit=50".to_string();
    if let Some(a) = after {
        path.push_str(&format!("&after={a}"));
    }
    request("GET", &path, None).await
}

/// Answered (or all) conversations — any order.
pub async fn list_answered(after: Option<&str>) -> Result<ConversationsPage, ApiError> {
    let mut path = "/conversations?status=answered&limit=50".to_string();
    if let Some(a) = after {
        path.push_str(&format!("&after={a}"));
    }
    request("GET", &path, None).await
}

/// A thread's messages from `after_seq` forward.
pub async fn list_messages(user_id: &str, after_seq: u64) -> Result<MessagesPage, ApiError> {
    let path = format!("/conversations/{user_id}/messages?after_seq={after_seq}&limit=200");
    request("GET", &path, None).await
}

/// Send an expert reply. Returns the assigned `seq`.
pub async fn reply(user_id: &str, text: &str) -> Result<u64, ApiError> {
    let client_id = uuid::Uuid::now_v7().to_string();
    let body = serde_json::to_string(&ReplyReq { client_id: &client_id, text, kind: None, payload: None })
        .map_err(|e| ApiError::Other(e.to_string()))?;
    let v: serde_json::Value = request("POST", &format!("/conversations/{user_id}/reply"), Some(body)).await?;
    v.get("seq")
        .and_then(|s| s.as_u64())
        .ok_or_else(|| ApiError::Other("reply: missing seq".to_string()))
}

/// Send a typed data_request to the user: kind="data_request",
/// payload={"dataset": …}, text = the human-readable RU fallback. Returns the seq.
pub async fn reply_data_request(user_id: &str, dataset: &str, text: &str) -> Result<u64, ApiError> {
    let client_id = uuid::Uuid::now_v7().to_string();
    // payload travels as a JSON STRING (the worker stores it verbatim).
    let payload = serde_json::json!({ "dataset": dataset }).to_string();
    let body = serde_json::to_string(&ReplyReq {
        client_id: &client_id,
        text,
        kind: Some("data_request"),
        payload: Some(payload),
    })
    .map_err(|e| ApiError::Other(e.to_string()))?;
    let v: serde_json::Value = request("POST", &format!("/conversations/{user_id}/reply"), Some(body)).await?;
    v.get("seq")
        .and_then(|s| s.as_u64())
        .ok_or_else(|| ApiError::Other("reply_data_request: missing seq".to_string()))
}

/// Advance the expert-side read marker for a thread.
pub async fn mark_read(user_id: &str, seq: u64) -> Result<(), ApiError> {
    let body = serde_json::to_string(&serde_json::json!({ "seq": seq }))
        .map_err(|e| ApiError::Other(e.to_string()))?;
    let _: serde_json::Value = request("POST", &format!("/conversations/{user_id}/read"), Some(body)).await?;
    Ok(())
}

/// Result of GET /admin/me: whether the signed-in candidate is an approved
/// expert, and (if they have an outstanding request) the code to give the
/// operator. `approved=false` is normal data for a pending candidate — it is
/// NOT an auth error.
#[derive(Debug, Clone, Deserialize)]
pub struct AdminMe {
    pub approved: bool,
    #[serde(default)]
    pub code: Option<String>,
}

/// GET /admin/me (user JWT). Tells the UI whether to show the queue or the
/// request-access screen.
pub async fn admin_me() -> Result<AdminMe, ApiError> {
    request("GET", "/admin/me", None).await
}

/// POST /admin/request (user JWT). Creates (or returns the existing) short
/// request code for THIS authenticated candidate. Idempotent server-side.
pub async fn admin_request() -> Result<String, ApiError> {
    let v: serde_json::Value =
        request("POST", "/admin/request", Some("{}".to_string())).await?;
    v.get("code")
        .and_then(|c| c.as_str())
        .map(|s| s.to_string())
        .ok_or_else(|| ApiError::Other("admin_request: missing code".to_string()))
}

// ── Payments (payment-worker; same expert JWT, authorized via approved-admins) ──

/// A paid-but-unclaimed guest subscription: paid by a buyer on the landing whose
/// payment was never bound to an account. The operator refunds it manually in lava
/// (lava has no refund API), then marks it voided here.
#[derive(Debug, Clone, Deserialize)]
pub struct UnboundPayment {
    pub claim_id: String,
    #[serde(default)]
    pub provider: Option<String>,
    #[serde(default)]
    pub plan_id: Option<String>,
    #[serde(default)]
    pub contract_id: Option<String>,
    #[serde(default)]
    pub email: Option<String>,
    #[serde(default)]
    pub amount: Option<i64>,
    #[serde(default)]
    pub currency: Option<String>,
    /// ms-epoch the payment was confirmed paid (used for "waiting since").
    #[serde(default)]
    pub paid_at: Option<i64>,
}

#[derive(Deserialize)]
struct UnboundResp {
    unbound: Vec<UnboundPayment>,
}

/// GET /admin/unbound-payments (payment-worker). Paid-but-unclaimed subscriptions,
/// oldest-paid first — the operator's manual-refund worklist.
pub async fn unbound_payments() -> Result<Vec<UnboundPayment>, ApiError> {
    let r: UnboundResp =
        request_to(&payment_base()?, "GET", "/admin/unbound-payments", None).await?;
    Ok(r.unbound)
}

/// A paid user who hasn't set up durable access (no passkey) — «paid but can't get in yet».
#[derive(Debug, Clone, Deserialize)]
pub struct PaidNoAccess {
    #[serde(default)]
    pub user_id: Option<String>,
    #[serde(default)]
    pub tg_user_id: Option<i64>,
    #[serde(default)]
    pub tg_username: Option<String>,
    #[serde(default)]
    pub amount: Option<i64>,
    #[serde(default)]
    pub currency: Option<String>,
    #[serde(default)]
    pub paid_at: Option<i64>,
    #[serde(default)]
    pub created_at: Option<i64>,
}

#[derive(Deserialize)]
struct PaidNoAccessResp {
    users: Vec<PaidNoAccess>,
}

/// GET /admin/paid-no-access (payment-worker). Paid users with no passkey — the operator
/// nudges them to finish setting up access.
pub async fn paid_no_access() -> Result<Vec<PaidNoAccess>, ApiError> {
    let r: PaidNoAccessResp =
        request_to(&payment_base()?, "GET", "/admin/paid-no-access", None).await?;
    Ok(r.users)
}

/// A client-requested refund (access already revoked). The operator processes it
/// manually in lava using contract_id / email.
#[derive(Debug, Clone, Deserialize)]
pub struct RefundRequest {
    #[serde(default)]
    pub user_id: String,
    #[serde(default)]
    pub amount: i64,
    #[serde(default)]
    pub currency: String,
    #[serde(default)]
    pub contract_id: Option<String>,
    #[serde(default)]
    pub email: Option<String>,
    #[serde(default)]
    pub days_left: Option<i64>,
    #[serde(default)]
    pub created_at: Option<i64>,
}

#[derive(Deserialize)]
struct RefundsResp {
    refunds: Vec<RefundRequest>,
}

/// GET /admin/refunds (payment-worker). Client refund requests, newest first.
pub async fn refund_requests() -> Result<Vec<RefundRequest>, ApiError> {
    let r: RefundsResp = request_to(&payment_base()?, "GET", "/admin/refunds", None).await?;
    Ok(r.refunds)
}

/// A receipt email caught at the buyer address (Email Routing → receipt-worker) and bound to
/// its payment. `amount` is minor units (×100). List view — no body text (see [`receipt_detail`]).
#[derive(Debug, Clone, Deserialize)]
pub struct Receipt {
    pub id: String,
    #[serde(default)]
    pub claim_id: Option<String>,
    #[serde(default)]
    pub amount: Option<i64>,
    #[serde(default)]
    pub currency: Option<String>,
    #[serde(default)]
    pub received_at: Option<i64>,
    #[serde(default)]
    pub message_id: Option<String>,
    #[serde(default)]
    pub tg_user_id: Option<i64>,
    #[serde(default)]
    pub tg_username: Option<String>,
    #[serde(default)]
    pub user_id: Option<String>,
    #[serde(default)]
    pub email: Option<String>,
    #[serde(default)]
    pub status: Option<String>,
    #[serde(default)]
    pub pdf_key: Option<String>,
}

#[derive(Deserialize)]
struct ReceiptsResp {
    receipts: Vec<Receipt>,
}

/// GET /admin/receipts (payment-worker). Caught receipts bound to payments, newest first.
pub async fn receipts() -> Result<Vec<Receipt>, ApiError> {
    let r: ReceiptsResp = request_to(&payment_base()?, "GET", "/admin/receipts", None).await?;
    Ok(r.receipts)
}

/// The FULL receipt (incl. `body_text`) by id — for the detail view.
#[derive(Debug, Clone, Deserialize)]
pub struct ReceiptFull {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub amount: Option<i64>,
    #[serde(default)]
    pub currency: Option<String>,
    #[serde(default)]
    pub received_at: Option<i64>,
    #[serde(default)]
    pub message_id: Option<String>,
    #[serde(default)]
    pub body_text: Option<String>,
    #[serde(default)]
    pub pdf_key: Option<String>,
}

#[derive(Deserialize)]
struct ReceiptDetailResp {
    #[serde(default)]
    found: bool,
    #[serde(default)]
    receipt: Option<ReceiptFull>,
}

/// GET /admin/receipt?id= (payment-worker) → the full receipt with its body, or None.
pub async fn receipt_detail(id: &str) -> Result<Option<ReceiptFull>, ApiError> {
    let enc = js_sys::encode_uri_component(id).as_string().unwrap_or_default();
    let r: ReceiptDetailResp =
        request_to(&payment_base()?, "GET", &format!("/admin/receipt?id={enc}"), None).await?;
    Ok(if r.found { r.receipt } else { None })
}


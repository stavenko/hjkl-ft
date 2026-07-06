use serde::Deserialize;
use worker::*;

use crate::types::{ConversationSummary, ConversationsPage};

const DEFAULT_LIMIT: i64 = 50;
const MAX_LIMIT: i64 = 200;

#[derive(Debug, Deserialize)]
struct ConvRow {
    user_id: String,
    preview: Option<String>,
    last_ts: Option<String>,
    last_seq: Option<i64>,
    pending_since: Option<String>,
    #[serde(default)]
    pending_seq: Option<i64>,
}

/// Existing row state read before a touch/clear. Single-threaded DO execution
/// makes this SELECT-then-write atomic, so these decisions are race-free.
#[derive(Debug, Deserialize)]
struct ExistingRow {
    pending_since: Option<String>,
    last_seq: Option<i64>,
}

#[derive(Debug, Deserialize)]
struct MetaRow {
    v: String,
}

#[derive(Debug, Deserialize)]
struct CodeRow {
    code: String,
}

#[derive(Debug, Deserialize)]
struct SubRow {
    sub: String,
}

impl From<ConvRow> for ConversationSummary {
    fn from(r: ConvRow) -> Self {
        ConversationSummary {
            user_id: r.user_id,
            preview: r.preview.unwrap_or_default(),
            last_ts: r.last_ts.unwrap_or_default(),
            last_seq: r.last_seq.unwrap_or(0) as u64,
            pending_since: r.pending_since,
        }
    }
}

/// Global singleton (`idFromName("index")`) tracking one row per conversation for
/// the expert queue. The worker maintains this — never the ConversationDO. The
/// touch/clear ops are idempotent + monotonic, so the worker can ALWAYS call them
/// (even after a deduped append) and a retry self-heals rather than corrupting the
/// queue. Arrival order is a strictly-increasing `pending_seq` (no millisecond ties).
#[durable_object]
pub struct ConversationIndexDO {
    state: worker::durable::State,
    // Required by the #[durable_object] new(state, env) signature; unused here.
    #[allow(dead_code)]
    env: Env,
}

impl ConversationIndexDO {
    fn ensure_schema(&self) -> Result<()> {
        let sql = self.state.storage().sql();
        sql.exec(
            "CREATE TABLE IF NOT EXISTS conversations (
                user_id       TEXT PRIMARY KEY,
                preview       TEXT,
                last_ts       TEXT,
                last_seq      INTEGER,
                pending_since TEXT,
                pending_seq   INTEGER
            )",
            None,
        )?;
        // Monotonic arrival counter: pending_seq for a NEW pending run is assigned
        // from next_pending_seq (start 1, only ever increases). Ordering the queue
        // by pending_seq gives true arrival order with no millisecond ties.
        sql.exec(
            "CREATE TABLE IF NOT EXISTS meta (
                k TEXT PRIMARY KEY,
                v TEXT NOT NULL
            )",
            None,
        )?;
        sql.exec(
            "INSERT OR IGNORE INTO meta(k,v) VALUES ('next_pending_seq','1')",
            None,
        )?;
        sql.exec(
            "CREATE INDEX IF NOT EXISTS idx_pending ON conversations(pending_seq)",
            None,
        )?;
        // Expert authorization (request-code + Cloudflare secret) — see the four
        // /admin-* ops below. These live in this GLOBAL singleton DO so an expert
        // can be approved at runtime with NO worker redeploy.
        sql.exec(
            "CREATE TABLE IF NOT EXISTS admins (
                sub         TEXT PRIMARY KEY,
                approved_at TEXT NOT NULL
            )",
            None,
        )?;
        // UNIQUE(sub) enforces idempotency at the storage layer: one pending
        // request (one code) per candidate.
        sql.exec(
            "CREATE TABLE IF NOT EXISTS admin_requests (
                code       TEXT PRIMARY KEY,
                sub        TEXT NOT NULL UNIQUE,
                created_at TEXT NOT NULL
            )",
            None,
        )?;
        Ok(())
    }

    /// Read+increment the monotonic arrival counter. Single-threaded DO execution
    /// makes the read-modify-write atomic.
    fn next_pending_seq(&self) -> Result<i64> {
        let sql = self.state.storage().sql();
        let row: MetaRow = sql
            .exec(
                "SELECT v FROM meta WHERE k = 'next_pending_seq'",
                None,
            )?
            .one()?;
        let n = row
            .v
            .parse::<i64>()
            .map_err(|e| Error::RustError(format!("parse next_pending_seq: {e}")))?;
        sql.exec(
            "UPDATE meta SET v = ? WHERE k = 'next_pending_seq'",
            vec![(n + 1).to_string().into()],
        )?;
        Ok(n)
    }

    /// Read the existing row for a conversation, if present.
    fn existing(&self, user_id: &str) -> Result<Option<ExistingRow>> {
        let sql = self.state.storage().sql();
        let rows: Vec<ExistingRow> = sql
            .exec(
                "SELECT pending_since, last_seq
                 FROM conversations WHERE user_id = ?",
                vec![user_id.into()],
            )?
            .to_array()?;
        Ok(rows.into_iter().next())
    }

    /// USER message. IDEMPOTENT + MONOTONIC, so the worker can always call it even
    /// after a deduped append.
    /// - pending_since/pending_seq are set ONLY when starting a NEW run (existing
    ///   pending_since IS NULL); an already-pending chat keeps its ORIGINAL position.
    /// - preview/last_ts/last_seq advance ONLY if new last_seq > existing last_seq,
    ///   so a deduped/older retry is a no-op.
    fn handle_touch_user(
        &self,
        user_id: &str,
        preview: &str,
        last_ts: &str,
        last_seq: i64,
    ) -> Result<Response> {
        let sql = self.state.storage().sql();

        match self.existing(user_id)? {
            None => {
                // Brand-new conversation: start a pending run with a fresh arrival seq.
                let pseq = self.next_pending_seq()?;
                sql.exec(
                    "INSERT INTO conversations(user_id,preview,last_ts,last_seq,pending_since,pending_seq)
                     VALUES (?,?,?,?,?,?)",
                    vec![
                        user_id.into(),
                        preview.into(),
                        last_ts.into(),
                        last_seq.into(),
                        last_ts.into(),
                        pseq.into(),
                    ],
                )?;
            }
            Some(ex) => {
                let existing_last = ex.last_seq.unwrap_or(0);
                // Start a new pending run only if not already pending.
                if ex.pending_since.is_none() {
                    let pseq = self.next_pending_seq()?;
                    sql.exec(
                        "UPDATE conversations SET pending_since = ?, pending_seq = ?
                         WHERE user_id = ?",
                        vec![last_ts.into(), pseq.into(), user_id.into()],
                    )?;
                }
                // Advance preview/last_* only on a genuinely newer message.
                if last_seq > existing_last {
                    sql.exec(
                        "UPDATE conversations SET preview = ?, last_ts = ?, last_seq = ?
                         WHERE user_id = ?",
                        vec![
                            preview.into(),
                            last_ts.into(),
                            last_seq.into(),
                            user_id.into(),
                        ],
                    )?;
                }
            }
        }
        Response::from_json(&serde_json::json!({ "ok": true }))
    }

    /// EXPERT reply. IDEMPOTENT + MONOTONIC; the `reply_seq` is the seq of the
    /// expert message this clear corresponds to.
    /// - Clear pending_since/pending_seq ONLY IF existing last_seq <= reply_seq, i.e.
    ///   no NEWER user message arrived after this reply. If the user re-opened
    ///   (last_seq > reply_seq) we DO NOT clear — a deduped/stale reply can't drop
    ///   the re-opened conversation out of the queue.
    /// - Advance preview/last_* only if reply_seq > existing last_seq.
    fn handle_clear_pending(
        &self,
        user_id: &str,
        preview: &str,
        last_ts: &str,
        reply_seq: i64,
    ) -> Result<Response> {
        let sql = self.state.storage().sql();

        // No row yet => nothing to clear (a reply with no prior user touch is a
        // degenerate case; treat as a no-op rather than fabricating a row).
        let ex = match self.existing(user_id)? {
            Some(ex) => ex,
            None => return Response::from_json(&serde_json::json!({ "ok": true })),
        };
        let existing_last = ex.last_seq.unwrap_or(0);

        if existing_last <= reply_seq {
            sql.exec(
                "UPDATE conversations SET pending_since = NULL, pending_seq = NULL
                 WHERE user_id = ?",
                vec![user_id.into()],
            )?;
        }
        if reply_seq > existing_last {
            sql.exec(
                "UPDATE conversations SET preview = ?, last_ts = ?, last_seq = ?
                 WHERE user_id = ?",
                vec![
                    preview.into(),
                    last_ts.into(),
                    reply_seq.into(),
                    user_id.into(),
                ],
            )?;
        }
        Response::from_json(&serde_json::json!({ "ok": true }))
    }

    fn handle_conversations(
        &self,
        status: &str,
        after: Option<&str>,
        limit: i64,
    ) -> Result<Response> {
        let limit = limit.clamp(1, MAX_LIMIT);
        let sql = self.state.storage().sql();

        let mut rows: Vec<ConvRow> = match status {
            "pending" => {
                // Oldest-waiting first by true arrival order (pending_seq is a strictly
                // increasing integer — no millisecond ties). Cursor = last pending_seq.
                match after {
                    Some(cursor) => {
                        let pseq = decode_cursor(cursor);
                        sql.exec(
                            "SELECT user_id,preview,last_ts,last_seq,pending_since,pending_seq
                             FROM conversations
                             WHERE pending_since IS NOT NULL
                               AND pending_seq > ?
                             ORDER BY pending_seq ASC
                             LIMIT ?",
                            vec![pseq.into(), (limit + 1).into()],
                        )?
                        .to_array()?
                    }
                    None => sql
                        .exec(
                            "SELECT user_id,preview,last_ts,last_seq,pending_since,pending_seq
                             FROM conversations
                             WHERE pending_since IS NOT NULL
                             ORDER BY pending_seq ASC
                             LIMIT ?",
                            vec![(limit + 1).into()],
                        )?
                        .to_array()?,
                }
            }
            "answered" => match after {
                Some(uid) => sql
                    .exec(
                        "SELECT user_id,preview,last_ts,last_seq,pending_since
                         FROM conversations
                         WHERE pending_since IS NULL AND user_id > ?
                         ORDER BY user_id ASC
                         LIMIT ?",
                        vec![uid.into(), (limit + 1).into()],
                    )?
                    .to_array()?,
                None => sql
                    .exec(
                        "SELECT user_id,preview,last_ts,last_seq,pending_since
                         FROM conversations
                         WHERE pending_since IS NULL
                         ORDER BY user_id ASC
                         LIMIT ?",
                        vec![(limit + 1).into()],
                    )?
                    .to_array()?,
            },
            other => return Response::error(format!("unknown status: {other}"), 400),
        };

        let has_more = rows.len() as i64 > limit;
        if has_more {
            rows.truncate(limit as usize);
        }

        let next_after = if has_more {
            rows.last().map(|r| match status {
                // Pending cursor is the arrival counter (integer), stringified.
                "pending" => r.pending_seq.unwrap_or(0).to_string(),
                _ => r.user_id.clone(),
            })
        } else {
            None
        };

        let conversations: Vec<ConversationSummary> =
            rows.into_iter().map(ConversationSummary::from).collect();

        Response::from_json(&ConversationsPage {
            conversations,
            next_after,
            has_more,
        })
    }

    // ---- expert authorization (request-code + secret) ----

    /// Candidate requests a short access code for their authenticated sub.
    /// IDEMPOTENT: an existing pending request returns the SAME code (one per sub,
    /// enforced by admin_requests.sub UNIQUE). The sub comes from the worker's
    /// authenticated /admin/request handler — never from an unauthenticated body.
    fn handle_admin_request(&self, sub: &str) -> Result<Response> {
        let sql = self.state.storage().sql();

        // Idempotent: return the existing code if this candidate already requested.
        let existing: Vec<CodeRow> = sql
            .exec(
                "SELECT code FROM admin_requests WHERE sub = ?",
                vec![sub.into()],
            )?
            .to_array()?;
        if let Some(r) = existing.into_iter().next() {
            return Response::from_json(&serde_json::json!({ "code": r.code }));
        }

        // Allocate a unique code; retry on the (rare) code-PK collision. Because we
        // SELECTed above and the DO is single-threaded, an INSERT failure here is a
        // code collision (the sub-UNIQUE path is impossible), so regenerate. We do
        // NOT silently swallow — exhausting retries returns a loud error.
        let mut code = generate_code()?;
        for _ in 0..8 {
            match sql.exec(
                "INSERT INTO admin_requests(code,sub,created_at) VALUES (?,?,?)",
                vec![code.clone().into(), sub.into(), now_rfc3339().into()],
            ) {
                Ok(_) => return Response::from_json(&serde_json::json!({ "code": code })),
                Err(_) => code = generate_code()?,
            }
        }
        Err(Error::RustError(
            "admin-request: could not allocate unique code".into(),
        ))
    }

    /// Resolve code -> sub from STORAGE (never from the caller) and approve that sub.
    /// 404 on an unknown code. The code is consumed (single-candidate) on approval.
    fn handle_admin_approve(&self, code: &str) -> Result<Response> {
        let sql = self.state.storage().sql();

        let rows: Vec<SubRow> = sql
            .exec(
                "SELECT sub FROM admin_requests WHERE code = ?",
                vec![code.into()],
            )?
            .to_array()?;
        let sub = match rows.into_iter().next() {
            Some(r) => r.sub,
            None => {
                return Response::from_json(&serde_json::json!({ "error": "unknown code" }))
                    .map(|r| r.with_status(404))
            }
        };

        // OR REPLACE => re-approving is idempotent.
        sql.exec(
            "INSERT OR REPLACE INTO admins(sub,approved_at) VALUES (?,?)",
            vec![sub.clone().into(), now_rfc3339().into()],
        )?;
        // Consume the request: the code is spent once approved (INVARIANT 5).
        sql.exec(
            "DELETE FROM admin_requests WHERE code = ?",
            vec![code.into()],
        )?;

        Response::from_json(&serde_json::json!({ "approved": true, "sub": sub }))
    }

    /// Consulted by auth_expert: is this sub a DO-approved admin?
    fn handle_admin_is_approved(&self, sub: &str) -> Result<Response> {
        let sql = self.state.storage().sql();
        let rows: Vec<SubRow> = sql
            .exec("SELECT sub FROM admins WHERE sub = ?", vec![sub.into()])?
            .to_array()?;
        let approved = rows.into_iter().next().is_some();
        Response::from_json(&serde_json::json!({ "approved": approved }))
    }

    /// Backs GET /admin/me: {approved, code|null} for the candidate's sub.
    fn handle_admin_get(&self, sub: &str) -> Result<Response> {
        let sql = self.state.storage().sql();
        let approved = !sql
            .exec("SELECT sub FROM admins WHERE sub = ?", vec![sub.into()])?
            .to_array::<SubRow>()?
            .is_empty();
        let code = sql
            .exec(
                "SELECT code FROM admin_requests WHERE sub = ?",
                vec![sub.into()],
            )?
            .to_array::<CodeRow>()?
            .into_iter()
            .next()
            .map(|r| r.code);
        Response::from_json(&serde_json::json!({ "approved": approved, "code": code }))
    }
}

/// Decode a pending cursor — the integer arrival counter (`pending_seq`) of the
/// last row of the previous page. A malformed cursor decodes to 0 (start).
fn decode_cursor(cursor: &str) -> i64 {
    cursor.parse::<i64>().unwrap_or(0)
}

/// 32-symbol unambiguous alphabet (drops I, O, 0, 1 so a candidate can read the
/// code aloud to the operator without confusion). 8 chars over 32 symbols = 40
/// bits of entropy.
const CODE_ALPHABET: &[u8] = b"ABCDEFGHJKLMNPQRSTUVWXYZ23456789";
const CODE_LEN: usize = 8;

/// CSPRNG access code. getrandom maps to crypto.getRandomValues in wasm.
/// 256 % 32 == 0, so the modulo is exactly uniform (no bias).
fn generate_code() -> Result<String> {
    let mut buf = [0u8; CODE_LEN];
    getrandom::getrandom(&mut buf).map_err(|e| Error::RustError(format!("getrandom: {e}")))?;
    let s: String = buf
        .iter()
        .map(|b| CODE_ALPHABET[(*b as usize) % CODE_ALPHABET.len()] as char)
        .collect();
    Ok(s)
}

fn now_rfc3339() -> String {
    chrono::Utc::now().to_rfc3339()
}

impl DurableObject for ConversationIndexDO {
    fn new(state: worker::durable::State, env: Env) -> Self {
        Self { state, env }
    }

    async fn fetch(&self, mut req: Request) -> Result<Response> {
        self.ensure_schema()?;
        let url = req.url()?;
        let path = url.path().to_string();

        match path.as_str() {
            "/touch-user" => {
                let body: serde_json::Value = req.json().await?;
                let user_id = body
                    .get("user_id")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| Error::RustError("missing user_id".into()))?;
                let preview = body.get("preview").and_then(|v| v.as_str()).unwrap_or("");
                let last_ts = body
                    .get("last_ts")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| Error::RustError("missing last_ts".into()))?;
                let last_seq = body
                    .get("last_seq")
                    .and_then(|v| v.as_i64())
                    .ok_or_else(|| Error::RustError("missing last_seq".into()))?;
                self.handle_touch_user(user_id, preview, last_ts, last_seq)
            }
            "/clear-pending" => {
                let body: serde_json::Value = req.json().await?;
                let user_id = body
                    .get("user_id")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| Error::RustError("missing user_id".into()))?;
                let preview = body.get("preview").and_then(|v| v.as_str()).unwrap_or("");
                let last_ts = body
                    .get("last_ts")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| Error::RustError("missing last_ts".into()))?;
                // reply_seq = seq of THIS expert reply (passed from lib.rs as last_seq).
                let reply_seq = body
                    .get("last_seq")
                    .and_then(|v| v.as_i64())
                    .ok_or_else(|| Error::RustError("missing last_seq".into()))?;
                self.handle_clear_pending(user_id, preview, last_ts, reply_seq)
            }
            "/conversations" => {
                let body: serde_json::Value = req.json().await?;
                let status = body
                    .get("status")
                    .and_then(|v| v.as_str())
                    .unwrap_or("pending");
                let after = body.get("after").and_then(|v| v.as_str());
                let limit = body
                    .get("limit")
                    .and_then(|v| v.as_i64())
                    .unwrap_or(DEFAULT_LIMIT);
                self.handle_conversations(status, after, limit)
            }
            "/admin-request" => {
                let body: serde_json::Value = req.json().await?;
                let sub = body
                    .get("sub")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| Error::RustError("missing sub".into()))?;
                self.handle_admin_request(sub)
            }
            "/admin-approve" => {
                let body: serde_json::Value = req.json().await?;
                let code = body
                    .get("code")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| Error::RustError("missing code".into()))?;
                self.handle_admin_approve(code)
            }
            "/admin-is-approved" => {
                let body: serde_json::Value = req.json().await?;
                let sub = body
                    .get("sub")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| Error::RustError("missing sub".into()))?;
                self.handle_admin_is_approved(sub)
            }
            "/admin-get" => {
                let body: serde_json::Value = req.json().await?;
                let sub = body
                    .get("sub")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| Error::RustError("missing sub".into()))?;
                self.handle_admin_get(sub)
            }
            _ => Response::error(format!("unknown DO path: {path}"), 404),
        }
    }
}

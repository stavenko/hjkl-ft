use serde::Deserialize;
use worker::*;

use crate::types::{AppendResult, Message, MessagesPage};

const DEFAULT_LIMIT: i64 = 50;
const MAX_LIMIT: i64 = 200;

/// Row as read back from the `messages` table. seq/INTEGER deserialises to i64;
/// the wire types convert to u64. seq stays far below 2^53 so this is lossless.
#[derive(Debug, Deserialize)]
struct MsgRow {
    seq: i64,
    client_id: String,
    sender: String,
    expert_id: Option<String>,
    text: String,
    created_at: String,
}

impl From<MsgRow> for Message {
    fn from(r: MsgRow) -> Self {
        Message {
            seq: r.seq as u64,
            client_id: r.client_id,
            sender: r.sender,
            expert_id: r.expert_id,
            text: r.text,
            created_at: r.created_at,
        }
    }
}

#[derive(Debug, Deserialize)]
struct SeqCreatedRow {
    seq: i64,
    created_at: String,
}

#[derive(Debug, Deserialize)]
struct MetaRow {
    v: String,
}

/// Per-user conversation, keyed `idFromName(user_id)`. Owns its own append-only
/// message log plus the user/expert read cursors. Single-threaded execution per
/// DO instance means the SELECT-then-INSERT in /append is race-free; the
/// `UNIQUE(client_id)` constraint is the hard idempotency backstop.
#[durable_object]
pub struct ConversationDO {
    state: worker::durable::State,
    // Required by the #[durable_object] new(state, env) signature; unused here.
    #[allow(dead_code)]
    env: Env,
}

impl ConversationDO {
    fn ensure_schema(&self) -> Result<()> {
        let sql = self.state.storage().sql();
        sql.exec(
            "CREATE TABLE IF NOT EXISTS messages (
                seq        INTEGER PRIMARY KEY,
                client_id  TEXT NOT NULL UNIQUE,
                sender     TEXT NOT NULL,
                expert_id  TEXT,
                text       TEXT NOT NULL,
                created_at TEXT NOT NULL
            )",
            None,
        )?;
        sql.exec(
            "CREATE TABLE IF NOT EXISTS meta (
                k TEXT PRIMARY KEY,
                v TEXT NOT NULL
            )",
            None,
        )?;
        sql.exec(
            "INSERT OR IGNORE INTO meta(k,v) VALUES
                ('next_seq','1'),
                ('user_read_seq','0'),
                ('expert_read_seq','0'),
                ('user_meta','{}')",
            None,
        )?;
        Ok(())
    }

    fn meta_i64(&self, key: &str) -> Result<i64> {
        let sql = self.state.storage().sql();
        let row: MetaRow = sql
            .exec("SELECT v FROM meta WHERE k = ?", vec![key.into()])?
            .one()?;
        row.v
            .parse::<i64>()
            .map_err(|e| Error::RustError(format!("parse meta {key}: {e}")))
    }

    fn set_meta(&self, key: &str, value: &str) -> Result<()> {
        let sql = self.state.storage().sql();
        sql.exec(
            "UPDATE meta SET v = ? WHERE k = ?",
            vec![value.into(), key.into()],
        )?;
        Ok(())
    }

    /// Idempotent append. Returns the existing seq/created_at with deduped=true
    /// when the client_id was already seen.
    fn handle_append(
        &self,
        client_id: &str,
        sender: &str,
        text: &str,
        expert_id: Option<&str>,
    ) -> Result<Response> {
        let sql = self.state.storage().sql();

        // 1. Idempotency: a known client_id returns its original row, no new write.
        let existing: Vec<SeqCreatedRow> = sql
            .exec(
                "SELECT seq, created_at FROM messages WHERE client_id = ?",
                vec![client_id.into()],
            )?
            .to_array()?;
        if let Some(row) = existing.into_iter().next() {
            return Response::from_json(&AppendResult {
                seq: row.seq as u64,
                created_at: row.created_at,
                deduped: true,
            });
        }

        // 2. Assign seq = next_seq, server timestamp (display only).
        let seq = self.meta_i64("next_seq")?;
        let created_at = chrono::Utc::now().to_rfc3339();

        // 3. Insert. UNIQUE(client_id) is the backstop: if a duplicate somehow
        //    slipped past the SELECT, re-read and return deduped instead of 500.
        //    NARROW: only a UNIQUE-constraint failure means "duplicate client_id";
        //    any OTHER insert error (disk, schema, etc.) must SURFACE, not be
        //    swallowed as a fake dedup (repo policy: never swallow real errors).
        let insert = sql.exec(
            "INSERT INTO messages(seq,client_id,sender,expert_id,text,created_at)
             VALUES (?,?,?,?,?,?)",
            vec![
                seq.into(),
                client_id.into(),
                sender.into(),
                expert_id.into(),
                text.into(),
                created_at.clone().into(),
            ],
        );
        if let Err(e) = insert {
            let msg = e.to_string();
            let is_unique = msg.contains("UNIQUE") || msg.contains("unique");
            if !is_unique {
                // Genuine error — fail loudly.
                return Err(e);
            }
            // Duplicate client_id raced past the SELECT: re-read and return deduped.
            let row: SeqCreatedRow = sql
                .exec(
                    "SELECT seq, created_at FROM messages WHERE client_id = ?",
                    vec![client_id.into()],
                )?
                .one()?;
            return Response::from_json(&AppendResult {
                seq: row.seq as u64,
                created_at: row.created_at,
                deduped: true,
            });
        }

        // 4. Advance next_seq.
        self.set_meta("next_seq", &(seq + 1).to_string())?;

        Response::from_json(&AppendResult {
            seq: seq as u64,
            created_at,
            deduped: false,
        })
    }

    fn handle_list(&self, after_seq: i64, limit: i64) -> Result<Response> {
        let limit = limit.clamp(1, MAX_LIMIT);
        let sql = self.state.storage().sql();
        // Fetch limit+1 to detect has_more.
        let mut rows: Vec<MsgRow> = sql
            .exec(
                "SELECT seq, client_id, sender, expert_id, text, created_at
                 FROM messages WHERE seq > ? ORDER BY seq ASC LIMIT ?",
                vec![after_seq.into(), (limit + 1).into()],
            )?
            .to_array()?;

        let has_more = rows.len() as i64 > limit;
        if has_more {
            rows.truncate(limit as usize);
        }
        let next_after_seq = rows.last().map(|r| r.seq as u64).unwrap_or(after_seq as u64);
        let messages: Vec<Message> = rows.into_iter().map(Message::from).collect();

        Response::from_json(&MessagesPage {
            messages,
            next_after_seq,
            has_more,
        })
    }

    /// Advance a read cursor forward only (monotonic). Never regresses.
    fn handle_read(&self, who: &str, seq: i64) -> Result<Response> {
        let key = match who {
            "user" => "user_read_seq",
            "expert" => "expert_read_seq",
            other => return Response::error(format!("unknown read role: {other}"), 400),
        };
        let current = self.meta_i64(key)?;
        if seq > current {
            self.set_meta(key, &seq.to_string())?;
        }
        Response::from_json(&serde_json::json!({ "ok": true }))
    }
}

impl DurableObject for ConversationDO {
    fn new(state: worker::durable::State, env: Env) -> Self {
        Self { state, env }
    }

    async fn fetch(&self, mut req: Request) -> Result<Response> {
        self.ensure_schema()?;
        let url = req.url()?;
        let path = url.path().to_string();

        match path.as_str() {
            "/append" => {
                let body: serde_json::Value = req.json().await?;
                let client_id = body
                    .get("client_id")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| Error::RustError("missing client_id".into()))?;
                let sender = body
                    .get("sender")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| Error::RustError("missing sender".into()))?;
                let text = body
                    .get("text")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| Error::RustError("missing text".into()))?;
                let expert_id = body.get("expert_id").and_then(|v| v.as_str());
                self.handle_append(client_id, sender, text, expert_id)
            }
            "/list" => {
                let body: serde_json::Value = req.json().await?;
                let after_seq = body.get("after_seq").and_then(|v| v.as_i64()).unwrap_or(0);
                let limit = body
                    .get("limit")
                    .and_then(|v| v.as_i64())
                    .unwrap_or(DEFAULT_LIMIT);
                self.handle_list(after_seq, limit)
            }
            "/read" => {
                let body: serde_json::Value = req.json().await?;
                let who = body
                    .get("who")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| Error::RustError("missing who".into()))?;
                let seq = body
                    .get("seq")
                    .and_then(|v| v.as_i64())
                    .ok_or_else(|| Error::RustError("missing seq".into()))?;
                self.handle_read(who, seq)
            }
            _ => Response::error(format!("unknown DO path: {path}"), 404),
        }
    }
}

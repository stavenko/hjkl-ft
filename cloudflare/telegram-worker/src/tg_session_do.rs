use serde::Deserialize;
use worker::*;

fn now_ms() -> i64 {
    Date::now().as_millis() as i64
}

/// One row of the per-chat promo session. promo_code is nullable so a NULL
/// round-trips as JSON null (never ""/absent confusion).
#[derive(Debug, Deserialize)]
struct SessionRow {
    #[allow(dead_code)]
    chat_id: i64,
    promo_code: Option<String>,
}

/// One row of the claimId → {chat_id, secret} mapping. Stored so the paid push can
/// reach the user. The plaintext claim secret lives ONLY here in telegram-worker.
#[derive(Debug, Deserialize)]
struct ClaimRow {
    #[allow(dead_code)]
    claim_id: String,
    chat_id: i64,
    secret: String,
    notified_at: Option<i64>,
}

/// One row of the Mini App claimId → {tg_user_id, secret} mapping. The Mini App has no
/// chat_id, so ownership is keyed by the validated Telegram WebApp user.id.
#[derive(Debug, Deserialize)]
struct MiniappClaimRow {
    #[allow(dead_code)]
    claim_id: String,
    tg_user_id: i64,
    secret: String,
}

/// SQLite-backed Telegram session store. One global instance (idFromName("global")):
/// every op runs under the DO's single-threaded input gate, so UPSERTs are atomic.
#[durable_object]
pub struct TgSessionDO {
    state: worker::durable::State,
    #[allow(dead_code)]
    env: Env,
}

impl TgSessionDO {
    fn ensure_schema(&self) -> Result<()> {
        let sql = self.state.storage().sql();
        sql.exec(
            "CREATE TABLE IF NOT EXISTS sessions (
                chat_id     INTEGER PRIMARY KEY,
                promo_code  TEXT,
                updated_at  INTEGER NOT NULL
            )",
            None,
        )?;
        sql.exec(
            "CREATE TABLE IF NOT EXISTS claims (
                claim_id    TEXT PRIMARY KEY,
                chat_id     INTEGER NOT NULL,
                secret      TEXT NOT NULL,
                created_at  INTEGER NOT NULL,
                notified_at INTEGER
            )",
            None,
        )?;
        // Mini App claims keyed by the Telegram WebApp user.id (NOT chat_id; the Mini
        // App has no chat). Used for the owner-gated /miniapp/status secret release.
        sql.exec(
            "CREATE TABLE IF NOT EXISTS miniapp_claims (
                claim_id    TEXT PRIMARY KEY,
                tg_user_id  INTEGER NOT NULL,
                secret      TEXT NOT NULL,
                created_at  INTEGER NOT NULL
            )",
            None,
        )?;
        Ok(())
    }

    // ---- session ops ----

    /// Last-typed promo wins (UPSERT).
    fn set_promo(&self, b: &serde_json::Value) -> Result<Response> {
        let chat_id = i64_field(b, "chatId")?;
        let promo_code = str_field(b, "promoCode")?;
        self.state.storage().sql().exec(
            "INSERT INTO sessions(chat_id, promo_code, updated_at) VALUES(?, ?, ?)
             ON CONFLICT(chat_id) DO UPDATE SET
               promo_code = excluded.promo_code,
               updated_at = excluded.updated_at",
            vec![chat_id.into(), promo_code.into(), now_ms().into()],
        )?;
        Response::from_json(&serde_json::json!({ "ok": true }))
    }

    /// Read stored promo for a chat. Absent row or NULL → JSON null.
    fn get_promo(&self, b: &serde_json::Value) -> Result<Response> {
        let chat_id = i64_field(b, "chatId")?;
        let row = self
            .state
            .storage()
            .sql()
            .exec(
                "SELECT chat_id, promo_code FROM sessions WHERE chat_id = ?",
                vec![chat_id.into()],
            )?
            .to_array::<SessionRow>()?
            .into_iter()
            .next();
        let promo = row.and_then(|r| r.promo_code).filter(|s| !s.is_empty());
        Response::from_json(&serde_json::json!({ "promoCode": promo }))
    }

    // ---- claim ops ----

    /// Store the claimId → {chat_id, secret} mapping. Idempotent (INSERT OR IGNORE):
    /// re-storing the same claimId is harmless.
    fn put_claim(&self, b: &serde_json::Value) -> Result<Response> {
        let claim_id = str_field(b, "claimId")?;
        let chat_id = i64_field(b, "chatId")?;
        let secret = str_field(b, "secret")?;
        self.state.storage().sql().exec(
            "INSERT OR IGNORE INTO claims(claim_id, chat_id, secret, created_at)
             VALUES(?, ?, ?, ?)",
            vec![claim_id.into(), chat_id.into(), secret.into(), now_ms().into()],
        )?;
        Response::from_json(&serde_json::json!({ "ok": true }))
    }

    /// Look up a claim. Unknown → {found:false}.
    fn get_claim(&self, b: &serde_json::Value) -> Result<Response> {
        let claim_id = str_field(b, "claimId")?;
        let row = self
            .state
            .storage()
            .sql()
            .exec(
                "SELECT claim_id, chat_id, secret, notified_at FROM claims WHERE claim_id = ?",
                vec![claim_id.into()],
            )?
            .to_array::<ClaimRow>()?
            .into_iter()
            .next();
        match row {
            Some(r) => Response::from_json(&serde_json::json!({
                "chatId": r.chat_id,
                "secret": r.secret,
                "notifiedAt": r.notified_at,
            })),
            None => Response::from_json(&serde_json::json!({ "found": false })),
        }
    }

    // ---- mini app claim ops (keyed by tg_user_id) ----

    /// Store the Mini App claimId → {tg_user_id, secret} mapping. Idempotent
    /// (INSERT OR IGNORE). The plaintext secret lives ONLY here and is NEVER logged.
    fn put_miniapp_claim(&self, b: &serde_json::Value) -> Result<Response> {
        let claim_id = str_field(b, "claimId")?;
        let tg_user_id = i64_field(b, "tgUserId")?;
        let secret = str_field(b, "secret")?;
        self.state.storage().sql().exec(
            "INSERT OR IGNORE INTO miniapp_claims(claim_id, tg_user_id, secret, created_at)
             VALUES(?, ?, ?, ?)",
            vec![claim_id.into(), tg_user_id.into(), secret.into(), now_ms().into()],
        )?;
        Response::from_json(&serde_json::json!({ "ok": true }))
    }

    /// Look up a Mini App claim. Unknown → {found:false}. The secret is returned only
    /// in-process to the owner-gated /miniapp/status path; never logged here.
    fn get_miniapp_claim(&self, b: &serde_json::Value) -> Result<Response> {
        let claim_id = str_field(b, "claimId")?;
        let row = self
            .state
            .storage()
            .sql()
            .exec(
                "SELECT claim_id, tg_user_id, secret FROM miniapp_claims WHERE claim_id = ?",
                vec![claim_id.into()],
            )?
            .to_array::<MiniappClaimRow>()?
            .into_iter()
            .next();
        match row {
            Some(r) => Response::from_json(&serde_json::json!({
                "tgUserId": r.tg_user_id,
                "secret": r.secret,
            })),
            None => Response::from_json(&serde_json::json!({ "found": false })),
        }
    }

    /// Record paid-notification delivery. Idempotent: re-stamping is harmless.
    fn mark_notified(&self, b: &serde_json::Value) -> Result<Response> {
        let claim_id = str_field(b, "claimId")?;
        self.state.storage().sql().exec(
            "UPDATE claims SET notified_at = ? WHERE claim_id = ?",
            vec![now_ms().into(), claim_id.into()],
        )?;
        Response::from_json(&serde_json::json!({ "ok": true }))
    }
}

fn str_field(b: &serde_json::Value, key: &str) -> Result<String> {
    b.get(key)
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .ok_or_else(|| Error::RustError(format!("missing {key}")))
}

fn i64_field(b: &serde_json::Value, key: &str) -> Result<i64> {
    b.get(key)
        .and_then(|v| v.as_i64())
        .ok_or_else(|| Error::RustError(format!("missing {key}")))
}

impl DurableObject for TgSessionDO {
    fn new(state: worker::durable::State, env: Env) -> Self {
        Self { state, env }
    }

    async fn fetch(&self, mut req: Request) -> Result<Response> {
        self.ensure_schema()?;
        let url = req.url()?;
        let path = url.path().to_string();
        let method = req.method();

        match (method, path.as_str()) {
            (Method::Post, "/session/set-promo") => {
                let b: serde_json::Value = req.json().await?;
                self.set_promo(&b)
            }
            (Method::Post, "/session/get-promo") => {
                let b: serde_json::Value = req.json().await?;
                self.get_promo(&b)
            }
            (Method::Post, "/claims/put") => {
                let b: serde_json::Value = req.json().await?;
                self.put_claim(&b)
            }
            (Method::Post, "/claims/get") => {
                let b: serde_json::Value = req.json().await?;
                self.get_claim(&b)
            }
            (Method::Post, "/claims/mark-notified") => {
                let b: serde_json::Value = req.json().await?;
                self.mark_notified(&b)
            }
            (Method::Post, "/miniapp/claims/put") => {
                let b: serde_json::Value = req.json().await?;
                self.put_miniapp_claim(&b)
            }
            (Method::Post, "/miniapp/claims/get") => {
                let b: serde_json::Value = req.json().await?;
                self.get_miniapp_claim(&b)
            }
            _ => Response::error("Not found", 404),
        }
    }
}

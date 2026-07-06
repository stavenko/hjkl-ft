use serde::Deserialize;
use worker::*;

const DAY_MS: i64 = 86_400_000;
const DEFAULT_PERIOD_DAYS: i64 = 30;

fn now_ms() -> i64 {
    Date::now().as_millis() as i64
}

/// One row of the guest paid-sub ledger. Nullable columns are Option so SQLite
/// NULLs round-trip (COALESCE correctness depends on real NULLs, never ""/0).
#[derive(Debug, Deserialize)]
struct ClaimRow {
    claim_id: String,
    secret_hash: String,
    provider: String,
    #[allow(dead_code)]
    plan_id: String,
    status: String, // "pending" | "paid" | "claimed" | "void"
    claimed_by: Option<String>,
    contract_id: Option<String>,
    email: Option<String>,
    #[allow(dead_code)]
    amount: Option<i64>,
    #[allow(dead_code)]
    currency: Option<String>,
    period_end: Option<i64>,
    paid_event_key: Option<String>,
    #[allow(dead_code)]
    #[serde(default)]
    pay_url: Option<String>,
    #[allow(dead_code)]
    created_at: i64,
    #[allow(dead_code)]
    paid_at: Option<i64>,
    #[allow(dead_code)]
    claimed_at: Option<i64>,
    #[allow(dead_code)]
    voided_at: Option<i64>,
    #[serde(default)]
    tg_user_id: Option<i64>,
    #[serde(default)]
    tg_code_hash: Option<String>,
    #[serde(default)]
    tg_code_expires: Option<i64>,
    // The universal account this payment belongs to (resolved at first touch). Used by the
    // webhook to activate the sub, and by the admin «paid but no credentials» worklist.
    #[serde(default)]
    user_id: Option<String>,
}

/// Single-column projection of PRAGMA table_info — detects whether a column already
/// exists before an idempotent ALTER (SQLite has no ADD COLUMN IF NOT EXISTS).
#[derive(Debug, Deserialize)]
struct PragmaCol {
    name: String,
}

/// SQLite-backed guest paid-sub ledger and THE atomic claim compare-and-set point
/// (MONEY-SAFETY #3, #5). One global instance (idFromName("claims")): every op runs
/// under the DO's single-threaded input gate, so read+write of a claim is atomic.
///
/// status lifecycle: 'pending' → 'paid' (signed webhook only) → 'claimed' (CAS).
/// 'void' is an irreversible tombstone (admin) — a late replayed paid event can
/// never resurrect it.
#[durable_object]
pub struct ClaimDO {
    state: worker::durable::State,
    #[allow(dead_code)]
    env: Env,
}

impl ClaimDO {
    fn ensure_schema(&self) -> Result<()> {
        let sql = self.state.storage().sql();
        sql.exec(
            "CREATE TABLE IF NOT EXISTS claims (
                claim_id        TEXT PRIMARY KEY,
                secret_hash     TEXT NOT NULL,
                provider        TEXT NOT NULL,
                plan_id         TEXT NOT NULL,
                status          TEXT NOT NULL,
                claimed_by      TEXT,
                contract_id     TEXT,
                email           TEXT,
                amount          INTEGER,
                currency        TEXT,
                period_end      INTEGER,
                paid_event_key  TEXT,
                created_at      INTEGER NOT NULL,
                paid_at         INTEGER,
                claimed_at      INTEGER,
                voided_at       INTEGER,
                tg_user_id      INTEGER,
                tg_username     TEXT,
                pay_url         TEXT,
                promo_code      TEXT,
                payment_method  TEXT
            )",
            None,
        )?;
        // Migrate tables created before later columns existed.
        let cols = sql
            .exec("PRAGMA table_info(claims)", None)?
            .to_array::<PragmaCol>()?;
        if !cols.iter().any(|c| c.name == "tg_user_id") {
            sql.exec("ALTER TABLE claims ADD COLUMN tg_user_id INTEGER", None)?;
        }
        if !cols.iter().any(|c| c.name == "tg_username") {
            sql.exec("ALTER TABLE claims ADD COLUMN tg_username TEXT", None)?;
        }
        // pay_url — the lava hosted checkout link, stored for status/reconciliation.
        if !cols.iter().any(|c| c.name == "pay_url") {
            sql.exec("ALTER TABLE claims ADD COLUMN pay_url TEXT", None)?;
        }
        // promo_code — the buyer's applied promo, stored for operator reconciliation.
        if !cols.iter().any(|c| c.name == "promo_code") {
            sql.exec("ALTER TABLE claims ADD COLUMN promo_code TEXT", None)?;
        }
        // payment_method — the chosen lava acquirer channel, stored for reconciliation.
        if !cols.iter().any(|c| c.name == "payment_method") {
            sql.exec("ALTER TABLE claims ADD COLUMN payment_method TEXT", None)?;
        }
        // user_id — the universal account this payment belongs to (resolved at first touch),
        // for the admin «paid but no credentials» worklist.
        if !cols.iter().any(|c| c.name == "user_id") {
            sql.exec("ALTER TABLE claims ADD COLUMN user_id TEXT", None)?;
        }
        sql.exec(
            "CREATE INDEX IF NOT EXISTS idx_claims_status ON claims(status)",
            None,
        )?;
        sql.exec(
            "CREATE INDEX IF NOT EXISTS idx_claims_contract ON claims(contract_id)",
            None,
        )?;
        sql.exec(
            "CREATE INDEX IF NOT EXISTS idx_claims_tg_user ON claims(tg_user_id)",
            None,
        )?;
        // Telegram-code fallback: one-time code (hash) + expiry for no-passkey devices.
        if !cols.iter().any(|c| c.name == "tg_code_hash") {
            sql.exec("ALTER TABLE claims ADD COLUMN tg_code_hash TEXT", None)?;
        }
        if !cols.iter().any(|c| c.name == "tg_code_expires") {
            sql.exec("ALTER TABLE claims ADD COLUMN tg_code_expires INTEGER", None)?;
        }
        sql.exec(
            "CREATE INDEX IF NOT EXISTS idx_claims_tg_code ON claims(tg_code_hash)",
            None,
        )?;
        // Refund requests (client asked for a refund; access already revoked). The
        // operator processes each manually in lava. One open request per user (PK).
        sql.exec(
            "CREATE TABLE IF NOT EXISTS refunds (
                user_id      TEXT PRIMARY KEY,
                amount       INTEGER NOT NULL,
                currency     TEXT NOT NULL,
                contract_id  TEXT,
                email        TEXT,
                days_left    INTEGER,
                created_at   INTEGER NOT NULL,
                status       TEXT NOT NULL DEFAULT 'requested'
            )",
            None,
        )?;
        // Telegram delivery side, moved here from telegram-worker's TgSessionDO so the
        // claim (payment) and its Telegram binding live in ONE store — no cross-worker
        // drift. One row per Mini App / bot claim. `tg_id` is the private-chat id (==
        // Mini App WebApp user.id); `secret` is the plaintext claim secret used to build
        // the onboarding link (`/onboard#claim=<claim_id>.<secret>`); `notified_at`
        // makes the paid → bot message idempotent.
        sql.exec(
            "CREATE TABLE IF NOT EXISTS tg_claims (
                claim_id     TEXT PRIMARY KEY,
                tg_id        INTEGER NOT NULL,
                secret       TEXT NOT NULL,
                created_at   INTEGER NOT NULL,
                notified_at  INTEGER
            )",
            None,
        )?;
        sql.exec(
            "CREATE INDEX IF NOT EXISTS idx_tg_claims_tg ON tg_claims(tg_id)",
            None,
        )?;
        // Named monotonic counters. `bill_seq` is bumped once per issued Telegram invoice
        // so its buyer/receipt email (`tg.<user>.<seq>@rcpt.renorma.app`) is unique per
        // invoice (lava rejects a repeated email that already has an active sub) AND
        // collision-proof — a globally increasing seq can never repeat, even if a Telegram
        // @username is released and reclaimed by a different account. Epoch-scoped (lives
        // in this DO instance; a DO_EPOCH bump resets it — acceptable, it wipes old state).
        sql.exec(
            "CREATE TABLE IF NOT EXISTS counters (
                name  TEXT PRIMARY KEY,
                value INTEGER NOT NULL
            )",
            None,
        )?;
        // Receipts caught at the buyer email (Email Routing → receipt-worker → payment-worker
        // /internal/receipt). One row per received receipt, bound to its payment (claim_id).
        // A payment can accrue several (initial + recurring renewals land on the SAME buyer
        // address). `amount`/`currency` = the total ON THE RECEIPT (authoritative, ×100 minor
        // units). `body_text` = full plaintext/HTML; `pdf_key` = R2 object key when the receipt
        // came as a PDF attachment. `message_id` = provider email Message-ID for idempotent
        // dedup (a UNIQUE index; SQLite treats NULLs as distinct, so a missing id never blocks).
        sql.exec(
            "CREATE TABLE IF NOT EXISTS receipts (
                id           TEXT PRIMARY KEY,
                claim_id     TEXT NOT NULL,
                message_id   TEXT,
                amount       INTEGER,
                currency     TEXT,
                body_text    TEXT,
                pdf_key      TEXT,
                received_at  INTEGER NOT NULL
            )",
            None,
        )?;
        sql.exec(
            "CREATE UNIQUE INDEX IF NOT EXISTS idx_receipts_msgid ON receipts(message_id)",
            None,
        )?;
        sql.exec(
            "CREATE INDEX IF NOT EXISTS idx_receipts_claim ON receipts(claim_id)",
            None,
        )?;
        // The synthetic buyer email is stored on the claim at create-pending so the
        // receipt-worker can resolve an incoming address → its payment deterministically
        // (not only after the paid webhook backfills it).
        sql.exec(
            "CREATE INDEX IF NOT EXISTS idx_claims_email ON claims(email)",
            None,
        )?;
        Ok(())
    }

    /// Store a received receipt, bound to its payment (claim). Idempotent on the provider
    /// Message-ID (INSERT OR IGNORE against the UNIQUE index) so an Email-Routing retry never
    /// double-inserts. WEBHOOK/INGESTION path only.
    fn add_receipt(&self, b: &serde_json::Value) -> Result<Response> {
        let id = str_field(b, "id")?;
        let claim_id = str_field(b, "claimId")?;
        let message_id = opt_str(b, "messageId");
        let amount = opt_i64(b, "amount");
        let currency = opt_str(b, "currency");
        let body_text = opt_str(b, "bodyText");
        let pdf_key = opt_str(b, "pdfKey");
        self.state.storage().sql().exec(
            "INSERT OR IGNORE INTO receipts
               (id, claim_id, message_id, amount, currency, body_text, pdf_key, received_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
            vec![
                id.into(),
                claim_id.into(),
                message_id.into(),
                amount.into(),
                currency.into(),
                body_text.into(),
                pdf_key.into(),
                now_ms().into(),
            ],
        )?;
        Response::from_json(&serde_json::json!({ "ok": true }))
    }

    /// Receipts for one payment (claim), newest first. Body text omitted from the list — the
    /// caller fetches a single receipt's full body/PDF on demand.
    fn receipts_by_claim(&self, b: &serde_json::Value) -> Result<Response> {
        let claim_id = str_field(b, "claimId")?;
        let rows: Vec<serde_json::Value> = self
            .state
            .storage()
            .sql()
            .exec(
                "SELECT id, claim_id, amount, currency, pdf_key, received_at
                   FROM receipts WHERE claim_id = ? ORDER BY received_at DESC",
                vec![claim_id.into()],
            )?
            .to_array::<serde_json::Value>()?;
        Response::from_json(&serde_json::json!({ "receipts": rows }))
    }

    /// All receipts belonging to a user's payments (any claim they claimed OR that resolved to
    /// their universal user_id), newest first — backs the «история чеков» in the app + admin.
    fn receipts_by_user(&self, b: &serde_json::Value) -> Result<Response> {
        let user_id = str_field(b, "userId")?;
        let rows: Vec<serde_json::Value> = self
            .state
            .storage()
            .sql()
            .exec(
                "SELECT r.id, r.claim_id, r.amount, r.currency, r.pdf_key, r.received_at
                   FROM receipts r JOIN claims c ON c.claim_id = r.claim_id
                  WHERE c.claimed_by = ? OR c.user_id = ?
                  ORDER BY r.received_at DESC",
                vec![user_id.clone().into(), user_id.into()],
            )?
            .to_array::<serde_json::Value>()?;
        Response::from_json(&serde_json::json!({ "receipts": rows }))
    }

    /// Admin: recent receipts across all payments, newest first, joined to their claim for
    /// context (payer identity + the synthetic address it landed on). Body text omitted —
    /// fetched per-receipt via `/receipt/get`.
    fn receipts_recent(&self) -> Result<Response> {
        let rows: Vec<serde_json::Value> = self
            .state
            .storage()
            .sql()
            .exec(
                "SELECT r.id, r.claim_id, r.amount, r.currency, r.pdf_key, r.received_at, r.message_id,
                        c.tg_user_id, c.tg_username, c.user_id, c.email, c.status
                   FROM receipts r JOIN claims c ON c.claim_id = r.claim_id
                  ORDER BY r.received_at DESC LIMIT 200",
                None,
            )?
            .to_array::<serde_json::Value>()?;
        Response::from_json(&serde_json::json!({ "receipts": rows }))
    }

    /// One receipt with its FULL body text + pdf key (for the detail view / PDF fetch).
    fn receipt_get(&self, b: &serde_json::Value) -> Result<Response> {
        let id = str_field(b, "id")?;
        let row = self
            .state
            .storage()
            .sql()
            .exec(
                "SELECT id, claim_id, message_id, amount, currency, body_text, pdf_key, received_at
                   FROM receipts WHERE id = ?",
                vec![id.into()],
            )?
            .to_array::<serde_json::Value>()?
            .into_iter()
            .next();
        match row {
            Some(r) => Response::from_json(&serde_json::json!({ "found": true, "receipt": r })),
            None => Response::from_json(&serde_json::json!({ "found": false })),
        }
    }

    /// Resolve an incoming receipt address → the payment it belongs to. Returns the claim id +
    /// the account it maps to (claimed_by, else the resolved user_id), so the ingestion path can
    /// bind the receipt. Unknown address → found:false. NEVER returns the secret hash.
    fn claim_by_email(&self, b: &serde_json::Value) -> Result<Response> {
        let email = str_field(b, "email")?;
        let row = self
            .state
            .storage()
            .sql()
            .exec(
                // COLLATE NOCASE: inbound recipient addresses arrive LOWERCASED (lava/CF
                // lowercases the local part), while the stored synthetic email may be mixed
                // case (base64url claimId) — match case-insensitively.
                "SELECT claim_id, claimed_by, user_id FROM claims
                   WHERE email = ? COLLATE NOCASE ORDER BY created_at DESC LIMIT 1",
                vec![email.into()],
            )?
            .to_array::<serde_json::Value>()?
            .into_iter()
            .next();
        match row {
            Some(r) => Response::from_json(&serde_json::json!({
                "found": true,
                "claimId": r.get("claim_id").and_then(|v| v.as_str()),
                "userId": r.get("claimed_by").and_then(|v| v.as_str())
                    .or_else(|| r.get("user_id").and_then(|v| v.as_str())),
            })),
            None => Response::from_json(&serde_json::json!({ "found": false })),
        }
    }

    /// Atomically bump and return a named counter. Runs under the DO's single-threaded
    /// input gate (no await between the two statements), so the increment + read are one
    /// race-free turn — no RETURNING needed.
    fn next_counter(&self, name: &str) -> Result<Response> {
        let sql = self.state.storage().sql();
        sql.exec(
            "INSERT INTO counters(name, value) VALUES(?, 1)
             ON CONFLICT(name) DO UPDATE SET value = value + 1",
            vec![name.into()],
        )?;
        let value = sql
            .exec("SELECT value FROM counters WHERE name = ?", vec![name.into()])?
            .to_array::<serde_json::Value>()?
            .into_iter()
            .next()
            .and_then(|r| r.get("value").and_then(|v| v.as_i64()))
            .ok_or_else(|| Error::RustError("counter read failed".into()))?;
        Response::from_json(&serde_json::json!({ "value": value }))
    }

    /// Record a refund request (client-initiated; access already revoked on the sub).
    /// One open request per user — a repeat replaces the prior row.
    fn add_refund(&self, b: &serde_json::Value) -> Result<Response> {
        let user_id = str_field(b, "userId")?;
        let amount = opt_i64(b, "amount").unwrap_or(0);
        let currency = opt_str(b, "currency").unwrap_or_else(|| "RUB".into());
        let contract_id = opt_str(b, "contractId");
        let email = opt_str(b, "email");
        let days_left = opt_i64(b, "daysLeft");
        self.state.storage().sql().exec(
            "INSERT INTO refunds(user_id, amount, currency, contract_id, email, days_left, created_at, status)
             VALUES(?, ?, ?, ?, ?, ?, ?, 'requested')
             ON CONFLICT(user_id) DO UPDATE SET
               amount=excluded.amount, currency=excluded.currency,
               contract_id=excluded.contract_id, email=excluded.email,
               days_left=excluded.days_left, created_at=excluded.created_at, status='requested'",
            vec![
                user_id.into(), amount.into(), currency.into(),
                contract_id.into(), email.into(), days_left.into(), now_ms().into(),
            ],
        )?;
        Response::from_json(&serde_json::json!({ "ok": true }))
    }

    /// Admin: all refund requests, newest first.
    fn list_refunds(&self) -> Result<Response> {
        let rows: Vec<serde_json::Value> = self
            .state
            .storage()
            .sql()
            .exec(
                "SELECT user_id, amount, currency, contract_id, email, days_left, created_at, status
                   FROM refunds ORDER BY created_at DESC",
                None,
            )?
            .to_array::<serde_json::Value>()?;
        Response::from_json(&serde_json::json!({ "refunds": rows }))
    }

    /// The account a claim is bound to (its lifecycle status + claimed_by user id), so
    /// the Mini App can look up that account's live subscription. Unknown → status "none".
    fn claimed_by(&self, b: &serde_json::Value) -> Result<Response> {
        let claim_id = str_field(b, "claimId")?;
        match self.row_by_claim(&claim_id)? {
            Some(r) => Response::from_json(&serde_json::json!({
                "status": r.status, "claimedBy": r.claimed_by,
            })),
            None => Response::from_json(&serde_json::json!({ "status": "none", "claimedBy": null })),
        }
    }

    /// The Telegram user id bound to an app account (via a claimed Mini App claim), so a
    /// subscription-status change (e.g. cancel) can be echoed to the bot. Newest claim
    /// wins. {tgUserId: <i64>|null}.
    fn tg_for_user(&self, b: &serde_json::Value) -> Result<Response> {
        let user_id = str_field(b, "userId")?;
        let row = self
            .state
            .storage()
            .sql()
            .exec(
                "SELECT tg_user_id FROM claims
                   WHERE claimed_by = ? AND tg_user_id IS NOT NULL
                   ORDER BY claimed_at DESC LIMIT 1",
                vec![user_id.into()],
            )?
            .to_array::<serde_json::Value>()?
            .into_iter()
            .next();
        let tg_user_id = row.and_then(|r| r.get("tg_user_id").and_then(|v| v.as_i64()));
        Response::from_json(&serde_json::json!({ "tgUserId": tg_user_id }))
    }

    fn row_by_claim(&self, claim_id: &str) -> Result<Option<ClaimRow>> {
        Ok(self
            .state
            .storage()
            .sql()
            .exec(
                "SELECT * FROM claims WHERE claim_id = ?",
                vec![claim_id.into()],
            )?
            .to_array::<ClaimRow>()?
            .into_iter()
            .next())
    }

    fn row_by_contract(&self, contract_id: &str) -> Result<Option<ClaimRow>> {
        Ok(self
            .state
            .storage()
            .sql()
            .exec(
                "SELECT * FROM claims WHERE contract_id = ?",
                vec![contract_id.into()],
            )?
            .to_array::<ClaimRow>()?
            .into_iter()
            .next())
    }

    // ---- ops ----

    /// Public poll: status of a claim by its (non-secret) claimId. Returns only
    /// the lifecycle status, NEVER the secret hash. Unknown claim → status "none".
    fn status_of(&self, b: &serde_json::Value) -> Result<Response> {
        let claim_id = str_field(b, "claimId")?;
        let status = match self.row_by_claim(&claim_id)? {
            Some(r) => r.status,
            None => "none".to_string(),
        };
        Response::from_json(&serde_json::json!({ "status": status }))
    }

    /// The newest NON-terminal claim for a Telegram user. Checkout uses it to refuse a
    /// duplicate purchase when the status is paid/claimed; the status endpoint uses it to
    /// report a pending invoice. Returns the claim's lifecycle status + (for a `pending`
    /// one) its stored payUrl. A `void` claim is terminal → skipped. NEVER returns a secret.
    fn active_by_tg(&self, b: &serde_json::Value) -> Result<Response> {
        let tg_user_id = opt_i64(b, "tgUserId")
            .ok_or_else(|| Error::RustError("missing tgUserId".into()))?;
        let row = self
            .state
            .storage()
            .sql()
            .exec(
                "SELECT claim_id, status, pay_url, created_at, contract_id, promo_code, currency, payment_method FROM claims
                   WHERE tg_user_id = ? AND status IN ('pending','paid','claimed')
                   ORDER BY created_at DESC LIMIT 1",
                vec![tg_user_id.into()],
            )?
            .to_array::<serde_json::Value>()?
            .into_iter()
            .next();
        match row {
            Some(r) => Response::from_json(&serde_json::json!({
                "status": r.get("status").and_then(|v| v.as_str()).unwrap_or("none"),
                "claimId": r.get("claim_id").and_then(|v| v.as_str()),
                "payUrl": r.get("pay_url").and_then(|v| v.as_str()),
                "createdAt": r.get("created_at").and_then(|v| v.as_i64()),
                "contractId": r.get("contract_id").and_then(|v| v.as_str()),
                "promoCode": r.get("promo_code").and_then(|v| v.as_str()),
                "currency": r.get("currency").and_then(|v| v.as_str()),
                "paymentMethod": r.get("payment_method").and_then(|v| v.as_str()),
            })),
            None => Response::from_json(&serde_json::json!({ "status": "none" })),
        }
    }

    // ---- Telegram binding (tg_claims) — moved here from telegram-worker ----

    /// Bind a claim to a Telegram user + store its plaintext secret. Idempotent.
    fn tg_put(&self, b: &serde_json::Value) -> Result<Response> {
        let claim_id = str_field(b, "claimId")?;
        let tg_id = opt_i64(b, "tgId").ok_or_else(|| Error::RustError("missing tgId".into()))?;
        let secret = str_field(b, "secret")?;
        self.state.storage().sql().exec(
            "INSERT OR IGNORE INTO tg_claims(claim_id, tg_id, secret, created_at) VALUES(?, ?, ?, ?)",
            vec![claim_id.into(), tg_id.into(), secret.into(), now_ms().into()],
        )?;
        Response::from_json(&serde_json::json!({ "ok": true }))
    }

    /// The Telegram binding for a claim (tg_id, secret, notified_at). Unknown → found:false.
    fn tg_get(&self, b: &serde_json::Value) -> Result<Response> {
        let claim_id = str_field(b, "claimId")?;
        let row = self
            .state
            .storage()
            .sql()
            .exec(
                "SELECT tg_id, secret, notified_at FROM tg_claims WHERE claim_id = ?",
                vec![claim_id.into()],
            )?
            .to_array::<serde_json::Value>()?
            .into_iter()
            .next();
        match row {
            Some(r) => Response::from_json(&serde_json::json!({
                "found": true,
                "tgId": r.get("tg_id").and_then(|v| v.as_i64()),
                "secret": r.get("secret").and_then(|v| v.as_str()),
                "notifiedAt": r.get("notified_at").and_then(|v| v.as_i64()),
            })),
            None => Response::from_json(&serde_json::json!({ "found": false })),
        }
    }

    /// This tg user's claims (newest first), with secrets — so the Mini App can find a
    /// paid one and build its onboarding link.
    fn tg_by_user(&self, b: &serde_json::Value) -> Result<Response> {
        let tg_id = opt_i64(b, "tgId").unwrap_or(-1);
        let rows: Vec<serde_json::Value> = self
            .state
            .storage()
            .sql()
            .exec(
                "SELECT claim_id AS claimId, secret FROM tg_claims WHERE tg_id = ? ORDER BY created_at DESC LIMIT 10",
                vec![tg_id.into()],
            )?
            .to_array::<serde_json::Value>()?;
        Response::from_json(&serde_json::json!({ "claims": rows }))
    }

    /// Stamp the paid-notification as delivered (idempotent bot message).
    fn tg_mark_notified(&self, b: &serde_json::Value) -> Result<Response> {
        let claim_id = str_field(b, "claimId")?;
        self.state.storage().sql().exec(
            "UPDATE tg_claims SET notified_at = ? WHERE claim_id = ?",
            vec![now_ms().into(), claim_id.into()],
        )?;
        Response::from_json(&serde_json::json!({ "ok": true }))
    }

    fn create_pending(&self, b: &serde_json::Value) -> Result<Response> {
        let claim_id = str_field(b, "claimId")?;
        let secret_hash = str_field(b, "secretHash")?;
        let provider = str_field(b, "provider")?;
        let plan_id = str_field(b, "planId")?;
        let contract_id = opt_str(b, "contractId");
        let amount = opt_i64(b, "amount");
        let currency = opt_str(b, "currency");
        let tg_user_id = opt_i64(b, "tgUserId");
        let tg_username = opt_str(b, "tgUsername");
        let pay_url = opt_str(b, "payUrl");
        let promo_code = opt_str(b, "promoCode");
        let payment_method = opt_str(b, "paymentMethod");
        let user_id = opt_str(b, "userId");
        // The synthetic buyer/receipt email, stored NOW so an incoming receipt maps to this
        // payment even before the paid webhook backfills the same value.
        let email = opt_str(b, "email");

        self.state.storage().sql().exec(
            "INSERT OR IGNORE INTO claims
               (claim_id, secret_hash, provider, plan_id, status, contract_id, amount, currency, created_at, tg_user_id, tg_username, pay_url, promo_code, payment_method, user_id, email)
             VALUES (?, ?, ?, ?, 'pending', ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
            vec![
                claim_id.into(),
                secret_hash.into(),
                provider.into(),
                plan_id.into(),
                contract_id.into(),
                amount.into(),
                currency.into(),
                now_ms().into(),
                tg_user_id.into(),
                tg_username.into(),
                pay_url.into(),
                promo_code.into(),
                payment_method.into(),
                user_id.into(),
                email.into(),
            ],
        )?;
        Response::from_json(&serde_json::json!({ "ok": true }))
    }

    /// WEBHOOK-ONLY (MONEY-SAFETY #2). Dedup + tombstone-guard + provider-anchored
    /// period; NEVER moves paid_at/period_end forward for an already-paid row.
    fn mark_paid(&self, b: &serde_json::Value) -> Result<Response> {
        let contract_id = str_field(b, "contractId")?;
        let event_key = str_field(b, "eventKey")?;
        let period_end = opt_i64(b, "periodEnd");
        let email = opt_str(b, "email");
        let amount = opt_i64(b, "amount");
        let currency = opt_str(b, "currency");

        let row = match self.row_by_contract(&contract_id)? {
            Some(r) => r,
            None => return Response::from_json(&serde_json::json!({ "ok": true, "mapped": false })),
        };

        // Tombstone guard (MONEY-SAFETY #4): a voided record can never be resurrected.
        if row.status == "void" {
            console_error!(
                "ClaimDO mark-paid on VOID claim claim_id={} contract={} eventKey={} — IGNORED (late replayed paid event)",
                row.claim_id,
                contract_id,
                event_key
            );
            return Response::from_json(&serde_json::json!({ "ok": true, "tombstoned": true }));
        }
        // Dedup (MONEY-SAFETY #4): same event replayed → no re-write.
        if row.paid_event_key.as_deref() == Some(event_key.as_str()) {
            return Response::from_json(&serde_json::json!({ "ok": true, "duplicate": true }));
        }

        let sql = self.state.storage().sql();
        if row.status == "pending" {
            sql.exec(
                "UPDATE claims SET status='paid',
                   period_end = COALESCE(?, period_end),
                   email      = COALESCE(?, email),
                   amount     = COALESCE(?, amount),
                   currency   = COALESCE(?, currency),
                   paid_at = ?, paid_event_key = ?
                 WHERE claim_id = ?",
                vec![
                    period_end.into(),
                    email.into(),
                    amount.into(),
                    currency.into(),
                    now_ms().into(),
                    event_key.into(),
                    row.claim_id.clone().into(),
                ],
            )?;
            return Response::from_json(&serde_json::json!({ "ok": true, "paid": true, "userId": row.user_id }));
        }
        if row.status == "paid" {
            // Different event key for an already-paid row: backfill only-null contact
            // fields; NEVER move paid_at/period_end forward (MONEY-SAFETY #4).
            sql.exec(
                "UPDATE claims SET
                   email    = COALESCE(email, ?),
                   amount   = COALESCE(amount, ?),
                   currency = COALESCE(currency, ?)
                 WHERE claim_id = ?",
                vec![
                    email.into(),
                    amount.into(),
                    currency.into(),
                    row.claim_id.clone().into(),
                ],
            )?;
            return Response::from_json(&serde_json::json!({ "ok": true, "alreadyPaid": true }));
        }
        // status == 'claimed' → already bound; renewals flow via SubscriptionDO.
        Response::from_json(&serde_json::json!({ "ok": true, "claimed": true }))
    }

    /// The ATOMIC compare-and-set (MONEY-SAFETY #3). One DO turn. Branch order EXACT.
    fn claim(&self, b: &serde_json::Value) -> Result<Response> {
        let claim_id = str_field(b, "claimId")?;
        let secret_hash = str_field(b, "secretHash")?;
        let user_id = str_field(b, "userId")?;

        let row = match self.row_by_claim(&claim_id)? {
            Some(r) => r,
            None => {
                return Response::from_json(&serde_json::json!({ "error": "claim_not_found" }))
                    .map(|r| r.with_status(404))
            }
        };
        if row.secret_hash != secret_hash {
            return Response::from_json(&serde_json::json!({ "error": "bad_secret" }))
                .map(|r| r.with_status(403));
        }
        if row.status == "void" {
            return Response::from_json(&serde_json::json!({ "error": "claim_void" }))
                .map(|r| r.with_status(409));
        }
        if row.status == "pending" {
            return Response::from_json(&serde_json::json!({ "error": "not_paid_yet" }))
                .map(|r| r.with_status(409));
        }
        if row.status == "claimed" {
            if row.claimed_by.as_deref() == Some(user_id.as_str()) {
                return Response::from_json(&serde_json::json!({
                    "ok": true,
                    "alreadyClaimed": true,
                    "periodEnd": row.period_end,
                    "provider": row.provider,
                    "contractId": row.contract_id,
                    "email": row.email,
                }));
            }
            // One sub = one account: hard-reject a different user.
            return Response::from_json(&serde_json::json!({ "error": "claimed_by_other" }))
                .map(|r| r.with_status(403));
        }
        // status == 'paid' → CAS to claimed.
        self.state.storage().sql().exec(
            "UPDATE claims SET status='claimed', claimed_by=?, claimed_at=? WHERE claim_id=?",
            vec![user_id.into(), now_ms().into(), claim_id.into()],
        )?;
        Response::from_json(&serde_json::json!({
            "ok": true,
            "periodEnd": row.period_end,
            "provider": row.provider,
            "contractId": row.contract_id,
            "email": row.email,
        }))
    }

    /// Admin: unbound (paid-but-never-claimed) payments for manual lava refund.
    /// Keeps the refund data (MONEY-SAFETY #8).
    fn unbound(&self) -> Result<Response> {
        let rows: Vec<serde_json::Value> = self
            .state
            .storage()
            .sql()
            .exec(
                "SELECT claim_id, provider, plan_id, status, contract_id, email, amount, currency, created_at, paid_at, tg_user_id, tg_username
                   FROM claims WHERE status='paid' ORDER BY paid_at ASC",
                None,
            )?
            .to_array::<serde_json::Value>()?;
        Response::from_json(&serde_json::json!({ "unbound": rows }))
    }

    /// Admin: paid payments that carry a universal user_id (new model). The caller cross-
    /// checks each user_id against auth-worker to surface «paid but no credentials».
    fn paid_with_user(&self) -> Result<Response> {
        let rows: Vec<serde_json::Value> = self
            .state
            .storage()
            .sql()
            .exec(
                "SELECT claim_id, user_id, amount, currency, created_at, paid_at, tg_user_id, tg_username
                   FROM claims WHERE status='paid' AND user_id IS NOT NULL ORDER BY paid_at ASC",
                None,
            )?
            .to_array::<serde_json::Value>()?;
        Response::from_json(&serde_json::json!({ "claims": rows }))
    }

    /// Admin: reconcile a Telegram user — «did they pay / did they bind an account?».
    /// Matches the given token against tg_username (case-insensitive, leading '@'
    /// stripped) OR the numeric tg_user_id. Returns every matching claim with its
    /// lifecycle status and claimed_by, newest first. NEVER returns the secret hash.
    fn by_tg(&self, b: &serde_json::Value) -> Result<Response> {
        let raw = str_field(b, "tg")?;
        let needle = raw.trim().trim_start_matches('@').trim().to_string();
        if needle.is_empty() {
            return Response::from_json(&serde_json::json!({ "claims": [] }));
        }
        let as_id: i64 = needle.parse().unwrap_or(-1);
        let rows: Vec<serde_json::Value> = self
            .state
            .storage()
            .sql()
            .exec(
                "SELECT claim_id, status, claimed_by, provider, plan_id, contract_id, email,
                        tg_user_id, tg_username, created_at, paid_at, claimed_at, voided_at, period_end
                   FROM claims
                  WHERE tg_user_id = ? OR LOWER(tg_username) = LOWER(?)
                  ORDER BY created_at DESC",
                vec![as_id.into(), needle.into()],
            )?
            .to_array::<serde_json::Value>()?;
        Response::from_json(&serde_json::json!({ "claims": rows }))
    }

    /// Admin: void a guest claim by claim id. Touches ONLY the guest record;
    /// refuses if already claimed/bound (MONEY-SAFETY #7).
    fn void(&self, b: &serde_json::Value) -> Result<Response> {
        let claim_id = str_field(b, "claimId")?;
        let row = match self.row_by_claim(&claim_id)? {
            Some(r) => r,
            None => {
                return Response::from_json(&serde_json::json!({ "error": "claim_not_found" }))
                    .map(|r| r.with_status(404))
            }
        };
        if row.status == "claimed" {
            return Response::from_json(&serde_json::json!({ "error": "already_claimed" }))
                .map(|r| r.with_status(409));
        }
        self.state.storage().sql().exec(
            "UPDATE claims SET status='void', voided_at=? WHERE claim_id=?",
            vec![now_ms().into(), claim_id.into()],
        )?;
        Response::from_json(&serde_json::json!({ "ok": true }))
    }

    /// Void by contract id — for a (rare) guest refund webhook. Same tombstone
    /// semantics; refuses a claimed/bound record.
    fn void_by_contract(&self, b: &serde_json::Value) -> Result<Response> {
        let contract_id = str_field(b, "contractId")?;
        let row = match self.row_by_contract(&contract_id)? {
            Some(r) => r,
            None => {
                return Response::from_json(&serde_json::json!({ "error": "claim_not_found" }))
                    .map(|r| r.with_status(404))
            }
        };
        if row.status == "claimed" {
            return Response::from_json(&serde_json::json!({ "error": "already_claimed" }))
                .map(|r| r.with_status(409));
        }
        self.state.storage().sql().exec(
            "UPDATE claims SET status='void', voided_at=? WHERE claim_id=?",
            vec![now_ms().into(), row.claim_id.into()],
        )?;
        Response::from_json(&serde_json::json!({ "ok": true }))
    }

    /// TEST-ONLY: write a row already 'paid' so a test claim can be CAS-claimed
    /// without a real webhook. Reachable ONLY via the worker's guarded /test/*
    /// route (impossible in prod: TEST_ENTITLEMENT unset).
    fn test_activate(&self, b: &serde_json::Value) -> Result<Response> {
        let claim_id = str_field(b, "claimId")?;
        let secret_hash = str_field(b, "secretHash")?;
        let provider = str_field(b, "provider")?;
        let plan_id = str_field(b, "planId")?;
        let now = now_ms();
        self.state.storage().sql().exec(
            "INSERT OR IGNORE INTO claims
               (claim_id, secret_hash, provider, plan_id, status, amount, currency, period_end, created_at, paid_at, paid_event_key)
             VALUES (?, ?, ?, ?, 'paid', 0, 'RUB', ?, ?, ?, ?)",
            vec![
                claim_id.clone().into(),
                secret_hash.into(),
                provider.into(),
                plan_id.into(),
                (now + DEFAULT_PERIOD_DAYS * DAY_MS).into(),
                now.into(),
                now.into(),
                format!("test:{claim_id}").into(),
            ],
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

fn opt_str(b: &serde_json::Value, key: &str) -> Option<String> {
    b.get(key).and_then(|v| v.as_str()).map(|s| s.to_string())
}

fn opt_i64(b: &serde_json::Value, key: &str) -> Option<i64> {
    b.get(key).and_then(|v| v.as_i64())
}

impl DurableObject for ClaimDO {
    fn new(state: worker::durable::State, env: Env) -> Self {
        Self { state, env }
    }

    async fn fetch(&self, mut req: Request) -> Result<Response> {
        self.ensure_schema()?;
        let url = req.url()?;
        let path = url.path().to_string();
        let method = req.method();

        match (method, path.as_str()) {
            (Method::Post, "/create-pending") => {
                let b: serde_json::Value = req.json().await?;
                self.create_pending(&b)
            }
            (Method::Post, "/mark-paid") => {
                let b: serde_json::Value = req.json().await?;
                self.mark_paid(&b)
            }
            (Method::Post, "/claim") => {
                let b: serde_json::Value = req.json().await?;
                self.claim(&b)
            }
            (Method::Post, "/status") => {
                let b: serde_json::Value = req.json().await?;
                self.status_of(&b)
            }
            (Method::Post, "/active-by-tg") => {
                let b: serde_json::Value = req.json().await?;
                self.active_by_tg(&b)
            }
            // No body: bump + return the global bill sequence (Telegram invoice email uniqueness).
            (Method::Post, "/next-bill-seq") => self.next_counter("bill_seq"),
            (Method::Post, "/receipt/add") => {
                let b: serde_json::Value = req.json().await?;
                self.add_receipt(&b)
            }
            (Method::Post, "/receipt/by-claim") => {
                let b: serde_json::Value = req.json().await?;
                self.receipts_by_claim(&b)
            }
            (Method::Post, "/receipt/by-user") => {
                let b: serde_json::Value = req.json().await?;
                self.receipts_by_user(&b)
            }
            (Method::Get, "/receipt/recent") => self.receipts_recent(),
            (Method::Post, "/receipt/get") => {
                let b: serde_json::Value = req.json().await?;
                self.receipt_get(&b)
            }
            (Method::Post, "/claim-by-email") => {
                let b: serde_json::Value = req.json().await?;
                self.claim_by_email(&b)
            }
            (Method::Post, "/tg/put") => {
                let b: serde_json::Value = req.json().await?;
                self.tg_put(&b)
            }
            (Method::Post, "/tg/get") => {
                let b: serde_json::Value = req.json().await?;
                self.tg_get(&b)
            }
            (Method::Post, "/tg/by-user") => {
                let b: serde_json::Value = req.json().await?;
                self.tg_by_user(&b)
            }
            (Method::Post, "/tg/mark-notified") => {
                let b: serde_json::Value = req.json().await?;
                self.tg_mark_notified(&b)
            }
            (Method::Get, "/unbound") => self.unbound(),
            (Method::Get, "/paid-with-user") => self.paid_with_user(),
            (Method::Post, "/refund-add") => {
                let b: serde_json::Value = req.json().await?;
                self.add_refund(&b)
            }
            (Method::Get, "/refunds") => self.list_refunds(),
            (Method::Post, "/tg-for-user") => {
                let b: serde_json::Value = req.json().await?;
                self.tg_for_user(&b)
            }
            (Method::Post, "/claimed-by") => {
                let b: serde_json::Value = req.json().await?;
                self.claimed_by(&b)
            }
            (Method::Post, "/by-tg") => {
                let b: serde_json::Value = req.json().await?;
                self.by_tg(&b)
            }
            (Method::Post, "/void") => {
                let b: serde_json::Value = req.json().await?;
                self.void(&b)
            }
            (Method::Post, "/void-by-contract") => {
                let b: serde_json::Value = req.json().await?;
                self.void_by_contract(&b)
            }
            (Method::Post, "/test-activate") => {
                let b: serde_json::Value = req.json().await?;
                self.test_activate(&b)
            }
            _ => Response::error("Not found", 404),
        }
    }
}

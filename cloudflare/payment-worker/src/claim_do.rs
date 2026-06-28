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
    created_at: i64,
    #[allow(dead_code)]
    paid_at: Option<i64>,
    #[allow(dead_code)]
    claimed_at: Option<i64>,
    #[allow(dead_code)]
    voided_at: Option<i64>,
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
                voided_at       INTEGER
            )",
            None,
        )?;
        sql.exec(
            "CREATE INDEX IF NOT EXISTS idx_claims_status ON claims(status)",
            None,
        )?;
        sql.exec(
            "CREATE INDEX IF NOT EXISTS idx_claims_contract ON claims(contract_id)",
            None,
        )?;
        Ok(())
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

    fn create_pending(&self, b: &serde_json::Value) -> Result<Response> {
        let claim_id = str_field(b, "claimId")?;
        let secret_hash = str_field(b, "secretHash")?;
        let provider = str_field(b, "provider")?;
        let plan_id = str_field(b, "planId")?;
        let contract_id = opt_str(b, "contractId");
        let amount = opt_i64(b, "amount");
        let currency = opt_str(b, "currency");

        self.state.storage().sql().exec(
            "INSERT OR IGNORE INTO claims
               (claim_id, secret_hash, provider, plan_id, status, contract_id, amount, currency, created_at)
             VALUES (?, ?, ?, ?, 'pending', ?, ?, ?, ?)",
            vec![
                claim_id.into(),
                secret_hash.into(),
                provider.into(),
                plan_id.into(),
                contract_id.into(),
                amount.into(),
                currency.into(),
                now_ms().into(),
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
            return Response::from_json(&serde_json::json!({ "ok": true, "paid": true }));
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
                "SELECT claim_id, provider, plan_id, status, contract_id, email, amount, currency, created_at, paid_at
                   FROM claims WHERE status='paid' ORDER BY paid_at ASC",
                None,
            )?
            .to_array::<serde_json::Value>()?;
        Response::from_json(&serde_json::json!({ "unbound": rows }))
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
            (Method::Get, "/unbound") => self.unbound(),
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

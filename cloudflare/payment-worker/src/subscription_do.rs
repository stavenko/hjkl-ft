use serde::{Deserialize, Serialize};
use worker::*;

const DAY_MS: i64 = 86_400_000;
const DEFAULT_PERIOD_DAYS: i64 = 30;

fn now_ms() -> i64 {
    Date::now().as_millis() as i64
}

/// Per-user subscription record (KV value under key "sub"). Option fields are
/// skipped when None so the stored shape mirrors the TS record.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubRecord {
    pub plan: String, // planId, or "paid" / "none"
    pub status: String, // "paid" | "cancelled" | "expired"
    pub start: i64,
    pub end: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub contract_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub email: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub no_renew: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub activate_key: Option<String>,
}

impl SubRecord {
    fn default_record() -> Self {
        // No trial (MONEY-SAFETY #6). A never-paid account has end=0 → active:false.
        // This default is NEVER persisted — only a real paid /activate writes it.
        SubRecord {
            plan: "none".into(),
            status: "expired".into(),
            start: 0,
            end: 0,
            provider: None,
            contract_id: None,
            email: None,
            no_renew: None,
            activate_key: None,
        }
    }
}

/// GATE CONTRACT: ai-worker / ocr-queue / story read `active`. `active` is
/// recomputed on every read against the wall clock. Do NOT rename these fields.
fn status_of(rec: &SubRecord) -> serde_json::Value {
    serde_json::json!({
        "plan": rec.plan,
        "status": rec.status,
        "start": rec.start,
        "end": rec.end,
        "active": now_ms() < rec.end,
        "provider": rec.provider,
        "contractId": rec.contract_id,
        "email": rec.email,
        "no_renew": rec.no_renew.unwrap_or(false),
    })
}

#[durable_object]
pub struct SubscriptionDO {
    state: worker::durable::State,
    #[allow(dead_code)]
    env: Env,
}

impl SubscriptionDO {
    async fn load(&self) -> Result<SubRecord> {
        Ok(self
            .state
            .storage()
            .get::<SubRecord>("sub")
            .await?
            .unwrap_or_else(SubRecord::default_record))
    }

    async fn save(&self, rec: &SubRecord) -> Result<()> {
        self.state.storage().put("sub", rec).await
    }
}

impl DurableObject for SubscriptionDO {
    fn new(state: worker::durable::State, env: Env) -> Self {
        Self { state, env }
    }

    async fn fetch(&self, mut req: Request) -> Result<Response> {
        let url = req.url()?;
        let path = url.path().to_string();
        let method = req.method();
        let mut rec = self.load().await?;

        match (method, path.as_str()) {
            (Method::Get, "/subscription") => Response::from_json(&status_of(&rec)),

            // Provider-driven: a payment succeeded → mark paid and extend.
            // Idempotent on activateKey (MONEY-SAFETY #4): a replayed claim/webhook
            // never re-extends. End is anchored to the provider-reported periodEnd,
            // NOT the handler wall-clock.
            (Method::Post, "/activate") => {
                let b: serde_json::Value = req.json().await?;
                let activate_key = b.get("activateKey").and_then(|v| v.as_str());
                // Replay of the SAME activation → no-op (do not move start/end forward).
                if let Some(ak) = activate_key {
                    if rec.activate_key.as_deref() == Some(ak) && rec.status == "paid" {
                        return Response::from_json(&status_of(&rec));
                    }
                }
                let now = now_ms();
                let period_end = b.get("periodEnd").and_then(|v| v.as_i64());
                rec.status = "paid".into();
                rec.plan = "paid".into();
                rec.start = now;
                rec.end = match period_end {
                    Some(pe) if pe > now => pe,
                    _ => now + DEFAULT_PERIOD_DAYS * DAY_MS,
                };
                if let Some(p) = b.get("provider").and_then(|v| v.as_str()) {
                    rec.provider = Some(p.to_string());
                }
                if let Some(c) = b.get("contractId").and_then(|v| v.as_str()) {
                    rec.contract_id = Some(c.to_string());
                }
                if let Some(e) = b.get("email").and_then(|v| v.as_str()) {
                    rec.email = Some(e.to_string());
                }
                rec.no_renew = Some(false);
                if let Some(ak) = activate_key {
                    rec.activate_key = Some(ak.to_string());
                }
                self.save(&rec).await?;
                Response::from_json(&status_of(&rec))
            }

            // Cancel auto-renew: stay active until `end` (or willExpireAt), then lapse.
            // Tolerate an empty/invalid body (treat as {}).
            (Method::Post, "/cancel") => {
                let b: serde_json::Value = req.json().await.unwrap_or(serde_json::json!({}));
                rec.no_renew = Some(true);
                rec.status = "cancelled".into();
                if let Some(pe) = b.get("periodEnd").and_then(|v| v.as_i64()) {
                    if pe > now_ms() {
                        rec.end = pe;
                    }
                }
                self.save(&rec).await?;
                Response::from_json(&status_of(&rec))
            }

            // Refund: revoke access immediately.
            (Method::Post, "/refund") => {
                rec.end = now_ms();
                rec.status = "expired".into();
                self.save(&rec).await?;
                Response::from_json(&status_of(&rec))
            }

            _ => Response::error("Not found", 404),
        }
    }
}

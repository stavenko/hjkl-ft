use worker::*;

fn now_ms() -> i64 {
    Date::now().as_millis() as i64
}

/// One global SQLite DO holding the invoices the mock has minted, so the pay-page
/// button and the /api/v2/invoices listing can look up an invoice by its contract
/// id (the id the mock returned from POST /api/v3/invoice, which the real payment
/// worker keys the webhook on). idFromName("global") at the worker layer.
#[durable_object]
pub struct InvoiceStoreDO {
    state: worker::durable::State,
    #[allow(dead_code)]
    env: Env,
}

impl InvoiceStoreDO {
    fn ensure_schema(&self) -> Result<()> {
        self.state.storage().sql().exec(
            "CREATE TABLE IF NOT EXISTS invoices (
                contract_id  TEXT PRIMARY KEY,
                email        TEXT NOT NULL,
                amount       REAL NOT NULL,
                currency     TEXT NOT NULL,
                offer_id     TEXT NOT NULL,
                created_at   INTEGER NOT NULL
            )",
            None,
        )?;
        Ok(())
    }

    fn insert(&self, b: &serde_json::Value) -> Result<Response> {
        let contract_id = b
            .get("contractId")
            .and_then(|v| v.as_str())
            .ok_or_else(|| Error::RustError("missing contractId".into()))?;
        let email = b.get("email").and_then(|v| v.as_str()).unwrap_or("");
        let amount = b.get("amount").and_then(|v| v.as_f64()).unwrap_or(0.0);
        let currency = b.get("currency").and_then(|v| v.as_str()).unwrap_or("RUB");
        let offer_id = b.get("offerId").and_then(|v| v.as_str()).unwrap_or("");

        self.state.storage().sql().exec(
            "INSERT OR REPLACE INTO invoices
               (contract_id, email, amount, currency, offer_id, created_at)
             VALUES (?, ?, ?, ?, ?, ?)",
            vec![
                contract_id.into(),
                email.into(),
                amount.into(),
                currency.into(),
                offer_id.into(),
                now_ms().into(),
            ],
        )?;
        Response::from_json(&serde_json::json!({ "ok": true }))
    }

    fn get_one(&self, contract_id: &str) -> Result<Response> {
        let rows: Vec<serde_json::Value> = self
            .state
            .storage()
            .sql()
            .exec(
                "SELECT contract_id, email, amount, currency, offer_id, created_at
                   FROM invoices WHERE contract_id = ?",
                vec![contract_id.into()],
            )?
            .to_array::<serde_json::Value>()?;
        match rows.into_iter().next() {
            Some(row) => Response::from_json(&row),
            None => Response::error("not found", 404),
        }
    }

    fn list(&self) -> Result<Response> {
        let rows: Vec<serde_json::Value> = self
            .state
            .storage()
            .sql()
            .exec(
                "SELECT contract_id, email, amount, currency, offer_id, created_at
                   FROM invoices ORDER BY created_at DESC LIMIT 500",
                None,
            )?
            .to_array::<serde_json::Value>()?;
        Response::from_json(&serde_json::json!({ "invoices": rows }))
    }
}

impl DurableObject for InvoiceStoreDO {
    fn new(state: worker::durable::State, env: Env) -> Self {
        Self { state, env }
    }

    async fn fetch(&self, mut req: Request) -> Result<Response> {
        self.ensure_schema()?;
        let url = req.url()?;
        let path = url.path().to_string();
        let method = req.method();

        match (method, path.as_str()) {
            (Method::Post, "/invoice") => {
                let b: serde_json::Value = req.json().await?;
                self.insert(&b)
            }
            (Method::Get, "/invoice") => {
                let cid = url
                    .query_pairs()
                    .find(|(k, _)| k == "contractId")
                    .map(|(_, v)| v.to_string())
                    .unwrap_or_default();
                if cid.is_empty() {
                    return Response::error("missing contractId", 400);
                }
                self.get_one(&cid)
            }
            (Method::Get, "/invoices") => self.list(),
            _ => Response::error("Not found", 404),
        }
    }
}

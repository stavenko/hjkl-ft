use worker::*;

/// UTC "YYYY-MM-DD" for the given epoch-ms instant. Neuro-token usage is bucketed
/// per day so the admin histogram is stable regardless of when the report runs.
fn utc_day(ms: i64) -> String {
    // toISOString() is always "YYYY-MM-DDT...Z" in UTC; the first 10 chars are the date.
    let iso = js_sys::Date::new(&wasm_bindgen::JsValue::from_f64(ms as f64))
        .to_iso_string()
        .as_string()
        .unwrap_or_default();
    iso.chars().take(10).collect()
}

/// SQLite-backed neuro-token usage ledger. ONE global instance (idFromName("usage")):
/// every write runs under the DO's single-threaded input gate, so the accumulate
/// upsert is race-free. Rows are (user_id, day, source) → summed tokens.
#[durable_object]
pub struct UsageDO {
    state: worker::durable::State,
    #[allow(dead_code)]
    env: Env,
}

impl UsageDO {
    fn ensure_schema(&self) -> Result<()> {
        self.state.storage().sql().exec(
            "CREATE TABLE IF NOT EXISTS usage (
                user_id TEXT NOT NULL,
                day     TEXT NOT NULL,
                source  TEXT NOT NULL,
                tokens  INTEGER NOT NULL DEFAULT 0,
                PRIMARY KEY (user_id, day, source)
            )",
            None,
        )?;
        Ok(())
    }

    /// Accumulate `tokens` for (user_id, today, source). Caller (lib.rs) has already
    /// validated tokens>0 and a non-empty userId; source is normalised to text|vision.
    fn add(&self, b: &serde_json::Value) -> Result<Response> {
        let user_id = b.get("userId").and_then(|v| v.as_str()).unwrap_or("");
        let tokens = b.get("tokens").and_then(|v| v.as_i64()).unwrap_or(0);
        let source = match b.get("source").and_then(|v| v.as_str()) {
            Some("vision") => "vision",
            _ => "text",
        };
        if user_id.is_empty() || tokens <= 0 {
            return Response::from_json(&serde_json::json!({ "ok": true }));
        }
        let day = utc_day(Date::now().as_millis() as i64);
        self.state.storage().sql().exec(
            "INSERT INTO usage(user_id, day, source, tokens) VALUES(?, ?, ?, ?)
             ON CONFLICT(user_id, day, source) DO UPDATE SET tokens = tokens + excluded.tokens",
            vec![
                user_id.into(),
                day.into(),
                source.into(),
                tokens.into(),
            ],
        )?;
        Response::from_json(&serde_json::json!({ "ok": true }))
    }

    /// Aggregate for the admin histogram: per-user totals + per-source split (DESC by
    /// tokens), per-day totals across all users (ASC by day), and the grand total.
    fn report(&self) -> Result<Response> {
        let sql = self.state.storage().sql();

        let users: Vec<serde_json::Value> = sql
            .exec(
                "SELECT user_id AS userId,
                        SUM(tokens) AS tokens,
                        SUM(CASE WHEN source = 'text'   THEN tokens ELSE 0 END) AS text,
                        SUM(CASE WHEN source = 'vision' THEN tokens ELSE 0 END) AS vision
                   FROM usage
                  GROUP BY user_id
                  ORDER BY tokens DESC",
                None,
            )?
            .to_array::<serde_json::Value>()?;

        let days: Vec<serde_json::Value> = sql
            .exec(
                "SELECT day, SUM(tokens) AS tokens
                   FROM usage
                  GROUP BY day
                  ORDER BY day ASC",
                None,
            )?
            .to_array::<serde_json::Value>()?;

        let total_rows: Vec<serde_json::Value> = sql
            .exec("SELECT COALESCE(SUM(tokens), 0) AS total FROM usage", None)?
            .to_array::<serde_json::Value>()?;
        let total = total_rows
            .first()
            .and_then(|r| r.get("total"))
            .and_then(|v| v.as_i64())
            .unwrap_or(0);

        Response::from_json(&serde_json::json!({
            "users": users,
            "days": days,
            "total": total,
        }))
    }
}

impl DurableObject for UsageDO {
    fn new(state: worker::durable::State, env: Env) -> Self {
        Self { state, env }
    }

    async fn fetch(&self, mut req: Request) -> Result<Response> {
        self.ensure_schema()?;
        let url = req.url()?;
        let path = url.path().to_string();
        let method = req.method();

        match (method, path.as_str()) {
            (Method::Post, "/add") => {
                let b: serde_json::Value = req.json().await?;
                self.add(&b)
            }
            (Method::Get, "/report") => self.report(),
            _ => Response::error("Not found", 404),
        }
    }
}

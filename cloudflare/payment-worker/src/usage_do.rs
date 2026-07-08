use std::time::Duration;

use worker::*;

/// $ per 1000 Cloudflare Neurons (Workers AI pricing). The price is NOT stored per
/// row — it is applied at read time so it can be re-derived if the tariff changes
/// (the raw tokens + neurons are what we persist). Returned to the admin in /report.
const PRICE_USD_PER_1K_NEURONS: f64 = 0.011;

/// How often the weekly rollup runs (rolling 7 days; weeks are bucketed by Monday).
const WEEK_MS: i64 = 7 * 86_400_000;

/// UTC "YYYY-MM-DD" for the given epoch-ms instant.
fn utc_day(ms: i64) -> String {
    let iso = js_sys::Date::new(&wasm_bindgen::JsValue::from_f64(ms as f64))
        .to_iso_string()
        .as_string()
        .unwrap_or_default();
    iso.chars().take(10).collect()
}

/// The Monday (UTC "YYYY-MM-DD") of the ISO week containing `day` ("YYYY-MM-DD").
/// Weeks bucket by Monday; YYYY-MM-DD strings compare chronologically, so a detail
/// row is in a COMPLETED week iff `day < current_week_start`.
fn week_start(day: &str) -> String {
    let d = js_sys::Date::new(&wasm_bindgen::JsValue::from_str(&format!("{day}T00:00:00Z")));
    let dow = d.get_utc_day() as i64; // 0=Sun .. 6=Sat
    let delta = if dow == 0 { 6 } else { dow - 1 }; // days since Monday
    let monday_ms = d.get_time() as i64 - delta * 86_400_000;
    utc_day(monday_ms)
}

/// SQLite-backed AI-usage ledger. ONE global instance (idFromName("usage")); every
/// write runs under the DO's single-threaded input gate, so accumulate upserts are
/// race-free. We persist the RAW inputs (in/out tokens, in/out neurons) so the price
/// is always re-derivable; the neuron figures are MICRO-neurons (neurons × 1e6) kept
/// as integers for exactness.
///
/// Two tables:
///   - `usage_detail` — per (user, day, source); short-term (the recent week).
///   - `usage_weekly` — per (week_start, user, source); long-term. A weekly alarm
///     rolls completed weeks out of detail into weekly, so detail stays small.
#[durable_object]
pub struct UsageDO {
    state: worker::durable::State,
    #[allow(dead_code)]
    env: Env,
}

impl UsageDO {
    fn ensure_schema(&self) -> Result<()> {
        let sql = self.state.storage().sql();
        sql.exec(
            "CREATE TABLE IF NOT EXISTS usage_detail (
                user_id     TEXT NOT NULL,
                day         TEXT NOT NULL,
                source      TEXT NOT NULL,
                in_tokens   INTEGER NOT NULL DEFAULT 0,
                out_tokens  INTEGER NOT NULL DEFAULT 0,
                in_neurons  INTEGER NOT NULL DEFAULT 0,
                out_neurons INTEGER NOT NULL DEFAULT 0,
                PRIMARY KEY (user_id, day, source)
            )",
            None,
        )?;
        sql.exec(
            "CREATE TABLE IF NOT EXISTS usage_weekly (
                week_start  TEXT NOT NULL,
                user_id     TEXT NOT NULL,
                source      TEXT NOT NULL,
                in_tokens   INTEGER NOT NULL DEFAULT 0,
                out_tokens  INTEGER NOT NULL DEFAULT 0,
                in_neurons  INTEGER NOT NULL DEFAULT 0,
                out_neurons INTEGER NOT NULL DEFAULT 0,
                PRIMARY KEY (week_start, user_id, source)
            )",
            None,
        )?;
        Ok(())
    }

    /// Accumulate one usage report into today's detail row. Body:
    /// { userId, source, inTokens, outTokens, inNeurons, outNeurons }.
    /// The caller (lib.rs) has authenticated the internal key; here we just validate
    /// shape. neurons are 0 for the vision source (on-prem GPU — not a CF cost).
    fn add(&self, b: &serde_json::Value) -> Result<Response> {
        let user_id = b.get("userId").and_then(|v| v.as_str()).unwrap_or("");
        let i64f = |k: &str| b.get(k).and_then(|v| v.as_i64()).unwrap_or(0).max(0);
        let in_tokens = i64f("inTokens");
        let out_tokens = i64f("outTokens");
        let in_neurons = i64f("inNeurons");
        let out_neurons = i64f("outNeurons");
        let source = match b.get("source").and_then(|v| v.as_str()) {
            Some("vision") => "vision",
            _ => "text",
        };
        if user_id.is_empty() || in_tokens + out_tokens <= 0 {
            return Response::from_json(&serde_json::json!({ "ok": true }));
        }
        let day = utc_day(Date::now().as_millis() as i64);
        self.state.storage().sql().exec(
            "INSERT INTO usage_detail(user_id, day, source, in_tokens, out_tokens, in_neurons, out_neurons)
             VALUES(?, ?, ?, ?, ?, ?, ?)
             ON CONFLICT(user_id, day, source) DO UPDATE SET
                in_tokens   = in_tokens   + excluded.in_tokens,
                out_tokens  = out_tokens  + excluded.out_tokens,
                in_neurons  = in_neurons  + excluded.in_neurons,
                out_neurons = out_neurons + excluded.out_neurons",
            vec![
                user_id.into(),
                day.into(),
                source.into(),
                in_tokens.into(),
                out_tokens.into(),
                in_neurons.into(),
                out_neurons.into(),
            ],
        )?;
        Response::from_json(&serde_json::json!({ "ok": true }))
    }

    /// Admin aggregate: the current week per-user (detail, DESC by neurons), the
    /// long-term weekly rows, and the price constant (so the admin renders the price
    /// AND can recompute it if the tariff moves).
    fn report(&self) -> Result<Response> {
        let sql = self.state.storage().sql();
        let cur_week = week_start(&utc_day(Date::now().as_millis() as i64));

        let week: Vec<serde_json::Value> = sql
            .exec(
                "SELECT user_id AS userId,
                        SUM(in_tokens)  AS inTokens,
                        SUM(out_tokens) AS outTokens,
                        SUM(in_neurons) AS inNeurons,
                        SUM(out_neurons) AS outNeurons
                   FROM usage_detail
                  WHERE day >= ?
                  GROUP BY user_id
                  ORDER BY (SUM(in_neurons) + SUM(out_neurons)) DESC",
                vec![cur_week.clone().into()],
            )?
            .to_array::<serde_json::Value>()?;

        let weekly: Vec<serde_json::Value> = sql
            .exec(
                "SELECT week_start AS weekStart, user_id AS userId,
                        SUM(in_tokens)  AS inTokens,
                        SUM(out_tokens) AS outTokens,
                        SUM(in_neurons) AS inNeurons,
                        SUM(out_neurons) AS outNeurons
                   FROM usage_weekly
                  GROUP BY week_start, user_id
                  ORDER BY week_start ASC",
                None,
            )?
            .to_array::<serde_json::Value>()?;

        Response::from_json(&serde_json::json!({
            "weekStart": cur_week,
            "week": week,
            "weekly": weekly,
            "priceUsdPer1kNeurons": PRICE_USD_PER_1K_NEURONS,
        }))
    }

    /// Roll every COMPLETED week (day < current Monday) out of detail into the weekly
    /// table, then delete those detail rows so short-term stays ~one week.
    fn rollup(&self) -> Result<()> {
        let sql = self.state.storage().sql();
        let cur_week = week_start(&utc_day(Date::now().as_millis() as i64));

        let rows: Vec<serde_json::Value> = sql
            .exec(
                "SELECT user_id, day, source, in_tokens, out_tokens, in_neurons, out_neurons
                   FROM usage_detail
                  WHERE day < ?",
                vec![cur_week.clone().into()],
            )?
            .to_array::<serde_json::Value>()?;

        for r in &rows {
            let get = |k: &str| r.get(k).and_then(|v| v.as_i64()).unwrap_or(0);
            let user_id = r.get("user_id").and_then(|v| v.as_str()).unwrap_or("");
            let day = r.get("day").and_then(|v| v.as_str()).unwrap_or("");
            let source = r.get("source").and_then(|v| v.as_str()).unwrap_or("text");
            let ws = week_start(day);
            sql.exec(
                "INSERT INTO usage_weekly(week_start, user_id, source, in_tokens, out_tokens, in_neurons, out_neurons)
                 VALUES(?, ?, ?, ?, ?, ?, ?)
                 ON CONFLICT(week_start, user_id, source) DO UPDATE SET
                    in_tokens   = in_tokens   + excluded.in_tokens,
                    out_tokens  = out_tokens  + excluded.out_tokens,
                    in_neurons  = in_neurons  + excluded.in_neurons,
                    out_neurons = out_neurons + excluded.out_neurons",
                vec![
                    ws.into(),
                    user_id.into(),
                    source.into(),
                    get("in_tokens").into(),
                    get("out_tokens").into(),
                    get("in_neurons").into(),
                    get("out_neurons").into(),
                ],
            )?;
        }

        // Prune the now-aggregated completed-week detail rows.
        sql.exec("DELETE FROM usage_detail WHERE day < ?", vec![cur_week.into()])?;
        Ok(())
    }

    /// Ensure the weekly rollup alarm is scheduled (rolling 7 days).
    async fn ensure_alarm(&self) -> Result<()> {
        if self.state.storage().get_alarm().await?.is_none() {
            self.state.storage().set_alarm(Duration::from_millis(WEEK_MS as u64)).await?;
        }
        Ok(())
    }
}

impl DurableObject for UsageDO {
    fn new(state: worker::durable::State, env: Env) -> Self {
        Self { state, env }
    }

    async fn fetch(&self, mut req: Request) -> Result<Response> {
        self.ensure_schema()?;
        self.ensure_alarm().await?;
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

    /// Weekly rollup: aggregate completed weeks into `usage_weekly`, prune detail,
    /// then reschedule for the next week.
    async fn alarm(&self) -> Result<Response> {
        self.ensure_schema()?;
        if let Err(e) = self.rollup() {
            console_error!("usage weekly rollup failed: {e:?}");
        }
        self.state.storage().set_alarm(Duration::from_millis(WEEK_MS as u64)).await?;
        Response::ok("")
    }
}

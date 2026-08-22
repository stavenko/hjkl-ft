use std::time::Duration;

use worker::*;

/// $ per 1000 Cloudflare Neurons (Workers AI pricing). The price is NOT stored per
/// row — it is applied at read time so it can be re-derived if the tariff changes
/// (the raw tokens + neurons are what we persist). Returned to the admin in /report.
const PRICE_USD_PER_1K_NEURONS: f64 = 0.011;

/// Версия схемы: 2 — у строк появилась МОДЕЛЬ. До неё сторонние модели считались
/// одной кучей «vision», и сказать, сколько съел конкретно qwen, было нечем.
const SCHEMA_VERSION: i64 = 2;

/// Тарифы сторонних моделей, $ за МИЛЛИОН токенов: `{"qwen3.8-27b": [вход, выход]}`.
/// Живут в переменной воркера `AI_PRICES`, а не в коде: прайс провайдера меняется
/// без нашего участия, и правка тарифа не должна требовать выката.
///
/// Модель без тарифа считается по нулю, и отчёт говорит об этом отдельным списком —
/// молча показать «$0.00» значило бы соврать, что расхода нет.
const PRICES_VAR: &str = "AI_PRICES";

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
///   - `usage_detail` — per (user, day, source, model); short-term (the recent week).
///   - `usage_weekly` — per (week_start, user, source, model); long-term. A weekly
///     alarm rolls completed weeks out of detail into weekly, so detail stays small.
///
/// МОДЕЛЬ входит в ключ: у стороннего провайдера счёт идёт по токенам конкретной
/// модели, и без неё нельзя ни оценить деньги, ни увидеть, какая именно съедает
/// квоту. Старые строки переезжают с пустой моделью — придумывать им имя нельзя.
#[durable_object]
pub struct UsageDO {
    state: worker::durable::State,
    env: Env,
}

impl UsageDO {
    fn ensure_schema(&self) -> Result<()> {
        let sql = self.state.storage().sql();
        sql.exec(
            "CREATE TABLE IF NOT EXISTS usage_meta (
                key   TEXT PRIMARY KEY,
                value TEXT NOT NULL
            )",
            None,
        )?;
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
        if self.schema_version()? < SCHEMA_VERSION {
            self.migrate_to_v2()?;
        }
        Ok(())
    }

    /// Версия схемы из `usage_meta`. Ноль — таблицы ещё первой редакции (или пусты).
    fn schema_version(&self) -> Result<i64> {
        let rows: Vec<serde_json::Value> = self
            .state
            .storage()
            .sql()
            .exec("SELECT value FROM usage_meta WHERE key = 'schema_version'", None)?
            .to_array::<serde_json::Value>()?;
        Ok(rows
            .first()
            .and_then(|r| r.get("value"))
            .and_then(|v| v.as_str())
            .and_then(|s| s.parse::<i64>().ok())
            .unwrap_or(0))
    }

    /// Добавить МОДЕЛЬ в ключ обеих таблиц. Перелить старое нельзя иначе как через
    /// новую таблицу: у SQLite первичный ключ не меняется `ALTER`ом. Уже накопленное
    /// переезжает с пустой моделью — какая именно за ним стояла, никто не знает.
    ///
    /// Один раз: версия схемы пишется в той же транзакции, что и переименование.
    fn migrate_to_v2(&self) -> Result<()> {
        let sql = self.state.storage().sql();
        for (table, key) in [
            ("usage_detail", "user_id, day, source, model"),
            ("usage_weekly", "week_start, user_id, source, model"),
        ] {
            let cols = if table == "usage_detail" {
                "user_id TEXT NOT NULL, day TEXT NOT NULL"
            } else {
                "week_start TEXT NOT NULL, user_id TEXT NOT NULL"
            };
            let carry = if table == "usage_detail" {
                "user_id, day, source"
            } else {
                "week_start, user_id, source"
            };
            sql.exec(
                &format!(
                    "CREATE TABLE IF NOT EXISTS {table}_v2 (
                        {cols},
                        source      TEXT NOT NULL,
                        model       TEXT NOT NULL DEFAULT '',
                        in_tokens   INTEGER NOT NULL DEFAULT 0,
                        out_tokens  INTEGER NOT NULL DEFAULT 0,
                        in_neurons  INTEGER NOT NULL DEFAULT 0,
                        out_neurons INTEGER NOT NULL DEFAULT 0,
                        PRIMARY KEY ({key})
                    )"
                ),
                None,
            )?;
            sql.exec(
                &format!(
                    "INSERT OR IGNORE INTO {table}_v2 ({carry}, model, in_tokens, out_tokens, in_neurons, out_neurons)
                     SELECT {carry}, '', in_tokens, out_tokens, in_neurons, out_neurons FROM {table}"
                ),
                None,
            )?;
            sql.exec(&format!("DROP TABLE {table}"), None)?;
            sql.exec(&format!("ALTER TABLE {table}_v2 RENAME TO {table}"), None)?;
        }
        sql.exec(
            "INSERT INTO usage_meta(key, value) VALUES('schema_version', ?)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            vec![SCHEMA_VERSION.to_string().into()],
        )?;
        Ok(())
    }

    /// Тарифы из переменной воркера. Пустая/битая — пустой справочник: считать
    /// деньги наугад хуже, чем честно показать «тариф не задан».
    fn prices(&self) -> std::collections::HashMap<String, (f64, f64)> {
        let raw = match self.env.var(PRICES_VAR) {
            Ok(v) => v.to_string(),
            Err(_) => return Default::default(),
        };
        let parsed: serde_json::Value = match serde_json::from_str(&raw) {
            Ok(v) => v,
            Err(e) => {
                console_error!("{PRICES_VAR} не разобран: {e}");
                return Default::default();
            }
        };
        let Some(obj) = parsed.as_object() else { return Default::default() };
        obj.iter()
            .filter_map(|(model, v)| {
                let a = v.as_array()?;
                Some((model.clone(), (a.first()?.as_f64()?, a.get(1)?.as_f64()?)))
            })
            .collect()
    }

    /// Accumulate one usage report into today's detail row. Body:
    /// { userId, source, model, inTokens, outTokens, inNeurons, outNeurons }.
    /// The caller (lib.rs) has authenticated the internal key; here we just validate
    /// shape. neurons are 0 for the vision/thirdparty sources (not a CF cost).
    fn add(&self, b: &serde_json::Value) -> Result<Response> {
        let user_id = b.get("userId").and_then(|v| v.as_str()).unwrap_or("");
        let i64f = |k: &str| b.get(k).and_then(|v| v.as_i64()).unwrap_or(0).max(0);
        let in_tokens = i64f("inTokens");
        let out_tokens = i64f("outTokens");
        let in_neurons = i64f("inNeurons");
        let out_neurons = i64f("outNeurons");
        let source = match b.get("source").and_then(|v| v.as_str()) {
            Some("vision") => "vision",
            Some("thirdparty") => "thirdparty",
            _ => "text",
        };
        // Имя модели — как прислали, без нормализации: по нему сходятся тариф
        // провайдера и наш счёт, и «поправленное» имя развело бы их.
        let model: String =
            b.get("model").and_then(|v| v.as_str()).unwrap_or("").trim().chars().take(64).collect();
        if user_id.is_empty() || in_tokens + out_tokens <= 0 {
            return Response::from_json(&serde_json::json!({ "ok": true }));
        }
        let day = utc_day(Date::now().as_millis() as i64);
        self.state.storage().sql().exec(
            "INSERT INTO usage_detail(user_id, day, source, model, in_tokens, out_tokens, in_neurons, out_neurons)
             VALUES(?, ?, ?, ?, ?, ?, ?, ?)
             ON CONFLICT(user_id, day, source, model) DO UPDATE SET
                in_tokens   = in_tokens   + excluded.in_tokens,
                out_tokens  = out_tokens  + excluded.out_tokens,
                in_neurons  = in_neurons  + excluded.in_neurons,
                out_neurons = out_neurons + excluded.out_neurons",
            vec![
                user_id.into(),
                day.into(),
                source.into(),
                model.into(),
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

        // ПО МОДЕЛЯМ — то, по чему выставляет счёт сторонний провайдер. Считается по
        // обеим таблицам сразу: свежая неделя лежит в detail, прошлые — в weekly, и
        // разделение на две «истории» здесь только мешало бы.
        let prices = self.prices();
        let by_model_rows: Vec<serde_json::Value> = sql
            .exec(
                "SELECT source, model,
                        SUM(in_tokens)  AS inTokens,
                        SUM(out_tokens) AS outTokens
                   FROM (SELECT source, model, in_tokens, out_tokens FROM usage_detail
                         UNION ALL
                         SELECT source, model, in_tokens, out_tokens FROM usage_weekly)
                  GROUP BY source, model
                  ORDER BY (SUM(in_tokens) + SUM(out_tokens)) DESC",
                None,
            )?
            .to_array::<serde_json::Value>()?;
        let by_model: Vec<serde_json::Value> = by_model_rows
            .iter()
            .map(|r| {
                let model = r.get("model").and_then(|v| v.as_str()).unwrap_or("");
                let get = |k: &str| r.get(k).and_then(|v| v.as_i64()).unwrap_or(0);
                let (ti, to) = (get("inTokens"), get("outTokens"));
                // Тариф известен → деньги; неизвестен → null, а не ноль: нулём мы бы
                // утверждали, что модель бесплатна.
                let usd = prices.get(model).map(|(pin, pout)| {
                    (ti as f64 * pin + to as f64 * pout) / 1_000_000.0
                });
                serde_json::json!({
                    "source": r.get("source").and_then(|v| v.as_str()).unwrap_or(""),
                    "model": model,
                    "inTokens": ti,
                    "outTokens": to,
                    "usd": usd,
                })
            })
            .collect();

        Response::from_json(&serde_json::json!({
            "weekStart": cur_week,
            "week": week,
            "weekly": weekly,
            "byModel": by_model,
            "priceUsdPer1kNeurons": PRICE_USD_PER_1K_NEURONS,
            "prices": prices.iter().map(|(m, (i, o))| (m.clone(), serde_json::json!([i, o])))
                .collect::<serde_json::Map<_, _>>(),
        }))
    }

    /// Roll every COMPLETED week (day < current Monday) out of detail into the weekly
    /// table, then delete those detail rows so short-term stays ~one week.
    fn rollup(&self) -> Result<()> {
        let sql = self.state.storage().sql();
        let cur_week = week_start(&utc_day(Date::now().as_millis() as i64));

        let rows: Vec<serde_json::Value> = sql
            .exec(
                "SELECT user_id, day, source, model, in_tokens, out_tokens, in_neurons, out_neurons
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
            let model = r.get("model").and_then(|v| v.as_str()).unwrap_or("");
            let ws = week_start(day);
            sql.exec(
                "INSERT INTO usage_weekly(week_start, user_id, source, model, in_tokens, out_tokens, in_neurons, out_neurons)
                 VALUES(?, ?, ?, ?, ?, ?, ?, ?)
                 ON CONFLICT(week_start, user_id, source, model) DO UPDATE SET
                    in_tokens   = in_tokens   + excluded.in_tokens,
                    out_tokens  = out_tokens  + excluded.out_tokens,
                    in_neurons  = in_neurons  + excluded.in_neurons,
                    out_neurons = out_neurons + excluded.out_neurons",
                vec![
                    ws.into(),
                    user_id.into(),
                    source.into(),
                    model.into(),
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
            // Erase one account's token accounting (detail + weekly rollup).
            (Method::Post, "/wipe-user") => {
                let b: serde_json::Value = req.json().await?;
                let user_id = b
                    .get("user_id")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| Error::RustError("missing user_id".into()))?;
                let sql = self.state.storage().sql();
                sql.exec(
                    "DELETE FROM usage_detail WHERE user_id = ?",
                    Some(vec![user_id.into()]),
                )?;
                sql.exec(
                    "DELETE FROM usage_weekly WHERE user_id = ?",
                    Some(vec![user_id.into()]),
                )?;
                Response::from_json(&serde_json::json!({ "ok": true }))
            }
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

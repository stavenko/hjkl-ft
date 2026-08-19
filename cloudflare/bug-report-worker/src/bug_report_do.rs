use serde::Serialize;
use worker::*;

fn now_ms() -> i64 {
    Date::now().as_millis() as i64
}

/// A stored bug report. Mirrors the TS `StoredReport` exactly: the client-supplied
/// report fields plus the worker-added `user` (JWT sub) and DO-added `id` +
/// `received_at`. Field order matches the TS object so the serialized JSON is the
/// same shape.
#[derive(Debug, Serialize)]
struct StoredReport {
    id: String,
    user: String,
    received_at: i64,
    title: String,
    area: String,
    steps_to_reproduce: String,
    expected: String,
    actual: String,
    severity: String,
    app_version: String,
}

/// One global SQLite DO (idFromName("global") at the worker layer) holding all bug
/// reports append-only. Same logical schema/columns as the TS KV record; rows are
/// listed newest-first (received_at DESC, id DESC), limit 500 — matching the TS
/// `list({ prefix:"report:", reverse:true, limit:500 })`.
#[durable_object]
pub struct BugReportDO {
    state: worker::durable::State,
    #[allow(dead_code)]
    env: Env,
}

impl BugReportDO {
    fn ensure_schema(&self) -> Result<()> {
        self.state.storage().sql().exec(
            "CREATE TABLE IF NOT EXISTS reports (
                id                  TEXT PRIMARY KEY,
                user                TEXT NOT NULL,
                received_at         INTEGER NOT NULL,
                title               TEXT NOT NULL,
                area                TEXT NOT NULL,
                steps_to_reproduce  TEXT NOT NULL,
                expected            TEXT NOT NULL,
                actual              TEXT NOT NULL,
                severity            TEXT NOT NULL,
                app_version         TEXT NOT NULL
            )",
            None,
        )?;
        // Неопознанная еда: копится здесь, а не в Analytics Engine, потому что из
        // воркера её надо ЧИТАТЬ — раз в сутки, чтобы отправить сводку. Аналитика
        // читается только снаружи, по API и с отдельным токеном; своя таблица снимает
        // и токен, и зависимость.
        self.state.storage().sql().exec(
            "CREATE TABLE IF NOT EXISTS unknown_food (
                id        TEXT PRIMARY KEY,
                subject   TEXT NOT NULL,
                user      TEXT NOT NULL,
                seen_at   INTEGER NOT NULL
            )",
            None,
        )?;
        Ok(())
    }

    fn str_or_default(b: &serde_json::Value, key: &str, default: &str) -> String {
        b.get(key)
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .unwrap_or_else(|| default.to_string())
    }

    fn insert(&self, b: &serde_json::Value) -> Result<Response> {
        let id = format!("bug_{}", uuid_v4());
        // Defaults mirror the TS `?? ...` fallbacks exactly.
        let user = b
            .get("user")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .ok_or_else(|| Error::RustError("missing user".into()))?;
        let rec = StoredReport {
            id: id.clone(),
            user,
            received_at: now_ms(),
            title: Self::str_or_default(b, "title", ""),
            area: Self::str_or_default(b, "area", "other"),
            steps_to_reproduce: Self::str_or_default(b, "steps_to_reproduce", ""),
            expected: Self::str_or_default(b, "expected", ""),
            actual: Self::str_or_default(b, "actual", ""),
            severity: Self::str_or_default(b, "severity", "medium"),
            app_version: Self::str_or_default(b, "app_version", ""),
        };

        self.state.storage().sql().exec(
            "INSERT INTO reports
               (id, user, received_at, title, area, steps_to_reproduce, expected, actual, severity, app_version)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
            vec![
                rec.id.clone().into(),
                rec.user.into(),
                rec.received_at.into(),
                rec.title.into(),
                rec.area.into(),
                rec.steps_to_reproduce.into(),
                rec.expected.into(),
                rec.actual.into(),
                rec.severity.into(),
                rec.app_version.into(),
            ],
        )?;

        Response::from_json(&serde_json::json!({ "id": id }))
    }

    /// Записать одну встречу с неопознанной едой.
    fn add_unknown(&self, b: &serde_json::Value) -> Result<Response> {
        let subject = Self::str_or_default(b, "subject", "");
        let user = Self::str_or_default(b, "user", "");
        if subject.is_empty() {
            return Response::error("missing subject", 400);
        }
        self.state.storage().sql().exec(
            "INSERT INTO unknown_food (id, subject, user, seen_at) VALUES (?, ?, ?, ?)",
            Some(vec![
                format!("uf_{}", uuid_v4()).into(),
                subject.into(),
                user.into(),
                (Date::now().as_millis() as i64).into(),
            ]),
        )?;
        Response::from_json(&serde_json::json!({ "ok": true }))
    }

    /// Сводка за последние `hours` часов: продукт, сколько раз и у скольких РАЗНЫХ
    /// людей. Людей считаем отдельно: один человек, добавивший продукт трижды, — не то
    /// же, что трое, споткнувшихся об одно имя, и пополнять словарь стоит по второму.
    fn unknown_digest(&self, hours: i64) -> Result<Response> {
        let since = Date::now().as_millis() as i64 - hours * 3_600_000;
        let rows: Vec<serde_json::Value> = self
            .state
            .storage()
            .sql()
            .exec(
                "SELECT subject, COUNT(*) AS n, COUNT(DISTINCT user) AS people
                   FROM unknown_food
                  WHERE seen_at > ?
                  GROUP BY subject
                  ORDER BY people DESC, n DESC
                  LIMIT 30",
                Some(vec![since.into()]),
            )?
            .to_array::<serde_json::Value>()?;
        // Старое чистим здесь же: таблица служебная, месяца истории хватает с запасом.
        let cutoff = Date::now().as_millis() as i64 - 30 * 24 * 3_600_000;
        self.state.storage().sql().exec(
            "DELETE FROM unknown_food WHERE seen_at < ?",
            Some(vec![cutoff.into()]),
        )?;
        Response::from_json(&serde_json::json!({ "foods": rows }))
    }

    fn list(&self) -> Result<Response> {
        let rows: Vec<serde_json::Value> = self
            .state
            .storage()
            .sql()
            .exec(
                "SELECT id, user, received_at, title, area, steps_to_reproduce, expected, actual, severity, app_version
                   FROM reports
                  ORDER BY received_at DESC, id DESC
                  LIMIT 500",
                None,
            )?
            .to_array::<serde_json::Value>()?;
        Response::from_json(&serde_json::json!({ "reports": rows }))
    }
}

impl DurableObject for BugReportDO {
    fn new(state: worker::durable::State, env: Env) -> Self {
        Self { state, env }
    }

    async fn fetch(&self, mut req: Request) -> Result<Response> {
        self.ensure_schema()?;
        let url = req.url()?;
        let path = url.path().to_string();
        let method = req.method();

        match (method, path.as_str()) {
            (Method::Post, "/report") => {
                let b: serde_json::Value = req.json().await?;
                self.insert(&b)
            }
            (Method::Get, "/reports") => self.list(),
            (Method::Post, "/unknown-food") => {
                let b: serde_json::Value = req.json().await?;
                self.add_unknown(&b)
            }
            (Method::Get, "/unknown-digest") => {
                let hours = url
                    .query_pairs()
                    .find(|(k, _)| k == "hours")
                    .and_then(|(_, v)| v.parse::<i64>().ok())
                    .unwrap_or(24);
                self.unknown_digest(hours)
            }
            // Erase one user's reports (the DO is global, so this is a targeted delete).
            (Method::Post, "/wipe-user") => {
                let b: serde_json::Value = req.json().await?;
                let user = b
                    .get("user_id")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| Error::RustError("missing user_id".into()))?;
                self.state.storage().sql().exec(
                    "DELETE FROM reports WHERE user = ?",
                    Some(vec![user.into()]),
                )?;
                self.state.storage().sql().exec(
                    "DELETE FROM unknown_food WHERE user = ?",
                    Some(vec![user.into()]),
                )?;
                Response::from_json(&serde_json::json!({ "ok": true }))
            }
            _ => Response::error("Not found", 404),
        }
    }
}

/// RFC 4122 v4 UUID (random), matching the shape of `crypto.randomUUID()` used by
/// the TS DO. Uses the JS-backed getrandom (wasm `js` feature).
fn uuid_v4() -> String {
    let mut bytes = [0u8; 16];
    getrandom::getrandom(&mut bytes).expect("getrandom failed");
    bytes[6] = (bytes[6] & 0x0f) | 0x40; // version 4
    bytes[8] = (bytes[8] & 0x3f) | 0x80; // variant 1
    format!(
        "{:02x}{:02x}{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
        bytes[0], bytes[1], bytes[2], bytes[3],
        bytes[4], bytes[5],
        bytes[6], bytes[7],
        bytes[8], bytes[9],
        bytes[10], bytes[11], bytes[12], bytes[13], bytes[14], bytes[15],
    )
}

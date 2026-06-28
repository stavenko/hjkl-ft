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

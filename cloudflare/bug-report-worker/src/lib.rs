// Bug-report intake.
//
// Authenticated app users file bug reports through the in-app support chat: the
// assistant gathers the details and calls its `file_bug_report` tool, which POSTs
// here. Every report is stored append-only in a single global `BugReportDO` (one
// instance, idFromName("global")), stamped with the reporting user's id (JWT `sub`)
// and a server received-at time. JWT-gated, so only signed-in app users can reach
// it. The admin read (GET /reports) is gated by ADMIN_KEY, not a user JWT.

use wasm_bindgen::JsValue;
use worker::*;

mod bug_report_do;
mod token;

pub use bug_report_do::BugReportDO;

// ── CORS ────────────────────────────────────────────────────────────────────
// Known origins only (no wildcard): the prod app + any renorma.app subdomain, the
// dev test env, and localhost for development. Mirrors the TS ALLOWED_ORIGIN_RE.
fn is_allowed_origin(origin: &str) -> bool {
    origin == "https://renorma.app"
        || (origin.starts_with("https://") && origin.ends_with(".renorma.app"))
        || origin == "https://renorma-fit-dev.pages.dev"
        || origin.starts_with("http://localhost")
        || origin.starts_with("http://127.0.0.1")
}

/// CORS headers attached to every JSON response (matches TS CORS_HEADERS). The
/// Access-Control-Allow-Origin / Vary:Origin is added by `apply_cors` at the edge.
fn cors_method_headers(headers: &Headers) {
    let _ = headers.set("Access-Control-Allow-Methods", "GET, POST, OPTIONS");
    let _ = headers.set("Access-Control-Allow-Headers", "Content-Type, Authorization, X-Admin-Key");
}

/// Echo a matching request Origin into Access-Control-Allow-Origin (no wildcard).
/// Always appends Vary: Origin. Mirrors the TS `applyCors`.
fn apply_cors(resp: Response, origin: &str) -> Result<Response> {
    let headers = Headers::new();
    for (k, v) in resp.headers() {
        let _ = headers.set(&k, &v);
    }
    let _ = headers.append("Vary", "Origin");
    if !origin.is_empty() && is_allowed_origin(origin) {
        let _ = headers.set("Access-Control-Allow-Origin", origin);
    }
    let status = resp.status_code();
    Ok(Response::from_body(resp.body().clone())?
        .with_headers(headers)
        .with_status(status))
}

// ── error helpers ─────────────────────────────────────────────────────────────
/// `{ "error": <message> }` with status + CORS method headers, mirroring the TS
/// `errorResponse` (which wraps `corsJson`).
fn error_response(message: &str, status: u16) -> Response {
    let resp = Response::from_json(&serde_json::json!({ "error": message }))
        .expect("serialize error")
        .with_status(status);
    let headers = resp.headers();
    cors_method_headers(headers);
    resp
}

/// Relay a DO response body + status, setting Content-Type + CORS method headers
/// (mirrors the TS `corsJson(await res.text(), res.status)`).
async fn cors_relay(mut res: Response) -> Result<Response> {
    let status = res.status_code();
    let text = res.text().await?;
    let headers = Headers::new();
    let _ = headers.set("Content-Type", "application/json");
    cors_method_headers(&headers);
    Ok(Response::ok(text)?.with_status(status).with_headers(headers))
}

// ── DO stub ────────────────────────────────────────────────────────────────────
fn bug_stub(env: &Env) -> Result<worker::durable::Stub> {
    env.durable_object("BUG_REPORT_DO")?
        .id_from_name("global")?
        .get_stub()
}

async fn do_get(stub: &worker::durable::Stub, path: &str) -> Result<Response> {
    stub.fetch_with_str(&format!("https://do{path}")).await
}

async fn do_post(
    stub: &worker::durable::Stub,
    path: &str,
    body: &serde_json::Value,
) -> Result<Response> {
    let body_str = serde_json::to_string(body)
        .map_err(|e| Error::RustError(format!("serialize DO body: {e}")))?;
    let headers = Headers::new();
    headers
        .set("Content-Type", "application/json")
        .map_err(|e| Error::RustError(format!("set header: {e}")))?;
    let mut init = RequestInit::new();
    init.with_method(Method::Post)
        .with_headers(headers)
        .with_body(Some(JsValue::from_str(&body_str)));
    let req = Request::new_with_init(&format!("https://do{path}"), &init)?;
    stub.fetch_with_request(req).await
}

// ── fail-loud secrets ──────────────────────────────────────────────────────────
/// Resolve every REQUIRED secret at the top of the fetch entry. On the first
/// failure: log loudly and return 503 so ANY request makes the misconfiguration
/// obvious (Workers have no separate startup — per-request is intended). Mirrors the
/// TS `requireSecrets` over the same names.
async fn require_secrets(env: &Env) -> std::result::Result<(), Response> {
    for name in ["JWT_SECRET", "ADMIN_KEY"] {
        if let Err(reason) = token::secret_or_var(env, name).await {
            console_error!("STARTUP MISCONFIG: {name}: {reason}");
            let body = format!("MISCONFIGURED: {name} — {reason}");
            return Err(
                Response::error(body, 503).unwrap_or_else(|_| Response::error("MISCONFIGURED", 503).unwrap()),
            );
        }
    }
    Ok(())
}

#[event(fetch)]
async fn main(req: Request, env: Env, _ctx: Context) -> Result<Response> {
    let origin = req
        .headers()
        .get("Origin")
        .ok()
        .flatten()
        .unwrap_or_default();

    // Preflight short-circuit (matches TS: 204 with CORS_HEADERS, before secrets).
    if req.method() == Method::Options {
        let headers = Headers::new();
        cors_method_headers(&headers);
        return apply_cors(Response::empty()?.with_headers(headers).with_status(204), &origin);
    }

    // Unauthenticated liveness probe (frontend `net` service). Wildcard CORS +
    // before secrets so it's a cheap, always-answerable 200 from any origin.
    if req.method() == Method::Get && req.url().map(|u| u.path() == "/health").unwrap_or(false) {
        let headers = Headers::new();
        let _ = headers.set("Access-Control-Allow-Origin", "*");
        let _ = headers.set("Cache-Control", "no-store");
        return Ok(Response::ok("ok")?.with_headers(headers));
    }

    if let Err(resp) = require_secrets(&env).await {
        // The TS does NOT route the 503 through applyCors (requireSecrets returns a
        // bare Response inside inner.fetch, which IS wrapped by applyCors). Match
        // that: wrap so a matching origin still gets ACAO.
        return apply_cors(resp, &origin);
    }

    let resp = match handle(req, &env).await {
        Ok(r) => r,
        Err(e) => error_response(&e.to_string(), 500),
    };
    apply_cors(resp, &origin)
}

async fn handle(mut req: Request, env: &Env) -> Result<Response> {
    let url = req.url()?;
    let path = url.path().to_string();
    let method = req.method();

    let stub = bug_stub(env)?;

    // ── Erase one user's reports. Reachable ONLY through a service binding: such a
    // fetch carries the dummy host the caller dialled, which no public request can
    // produce. Host + the shared internal key are both required. ──
    if method == Method::Post && path == "/internal/user-wipe" {
        if url.host_str() != Some("bug-report-worker") {
            return Response::error("Not found", 404);
        }
        let key = token::secret_or_var(env, "INTERNAL_PUSH_KEY")
            .await
            .map_err(Error::RustError)?;
        let provided = req
            .headers()
            .get("X-Internal-Key")
            .map_err(|e| Error::RustError(format!("{e}")))?
            .unwrap_or_default();
        if key.is_empty() || provided != key {
            return Response::error("unauthorized", 403);
        }
        let body: serde_json::Value = req.json().await.unwrap_or(serde_json::json!({}));
        let user_id = body.get("userId").and_then(|v| v.as_str()).unwrap_or_default();
        if user_id.is_empty() {
            return Response::error("missing userId", 400);
        }
        let mut resp = do_post(
            &stub,
            "/wipe-user",
            &serde_json::json!({ "user_id": user_id }),
        )
        .await?;
        if resp.status_code() != 200 {
            return Response::error(format!("wipe failed: {}", resp.text().await?), 502);
        }
        console_log!("bug-report: wiped reports of {user_id}");
        return Response::from_json(&serde_json::json!({ "ok": true }));
    }

    // ── Admin read: gather the collected reports. Gated by ADMIN_KEY (a developer
    // tool), NOT a user JWT — so one signed-in user can't read others' reports. ──
    if method == Method::Get && path == "/reports" {
        let admin_key = req
            .headers()
            .get("X-Admin-Key")
            .ok()
            .flatten()
            .unwrap_or_default();
        let expected = match token::secret_or_var(env, "ADMIN_KEY").await {
            Ok(k) => k,
            Err(reason) => {
                console_error!("ADMIN_KEY resolve failed: {reason}");
                return Ok(error_response(&format!("MISCONFIGURED: ADMIN_KEY — {reason}"), 503));
            }
        };
        if admin_key != expected {
            return Ok(error_response("Unauthorized", 401));
        }
        let res = do_get(&stub, "/reports").await?;
        return cors_relay(res).await;
    }

    // ── Everything else is app-JWT authed ──
    let auth_header = req
        .headers()
        .get("Authorization")
        .ok()
        .flatten()
        .unwrap_or_default();
    let bearer = auth_header.strip_prefix("Bearer ").unwrap_or("").to_string();

    let secret = match token::secret_or_var(env, "JWT_SECRET").await {
        Ok(s) => s,
        Err(reason) => {
            console_error!("JWT_SECRET resolve failed: {reason}");
            return Ok(error_response(&format!("MISCONFIGURED: JWT_SECRET — {reason}"), 503));
        }
    };

    if bearer.is_empty() || !token::verify_jwt(&bearer, &secret) {
        return Ok(error_response("Unauthorized", 401));
    }
    let user_id = match token::decode_jwt_sub(&bearer) {
        Some(u) => u,
        None => return Ok(error_response("Unauthorized", 401)),
    };

    if method == Method::Post && path == "/report" {
        let mut body: serde_json::Value = req.json().await?;
        if let Some(obj) = body.as_object_mut() {
            obj.insert("user".to_string(), serde_json::Value::String(user_id));
        }
        let res = do_post(&stub, "/report", &body).await?;
        return cors_relay(res).await;
    }

    if method == Method::Post && path == "/event" {
        return record_event(req, env, &user_id).await;
    }

    Ok(error_response("Not found", 404))
}

// ── Клиентские ошибки → Analytics Engine ─────────────────────────────────────
//
// Приложение шлёт сюда каждую ошибку, о которой сообщает человеку под
// треугольником. Раньше это оставалось на устройстве и умирало с перезагрузкой:
// узнать, что у десяти человек не определяется один и тот же продукт, было
// неоткуда.
//
// РАСКЛАДКА ТОЧКИ ДАННЫХ. В SQL столбцы позиционные (index1, blob1…blob20,
// double1…), имён у них нет — поменять порядок значит сломать все запросы и
// перемешать уже записанное. Менять только добавлением в конец.
//
//   index1  — код ошибки (устойчивый, считает клиент): по нему группируем
//   blob1   — вид: food.iron | food.kind | food.nutrients | planka.calories …
//   blob2   — к чему относится: название продукта
//   blob3   — техническая причина
//   blob4   — версия сборки
//   blob5   — платформа (ios_safari, android_chrome, …)
//   blob6   — user_id: сколько РАЗНЫХ людей задело, а не сколько раз стрельнуло
//   double1 — 1, чтобы складывать
const EVENT_DATASET: &str = "CLIENT_ERRORS";

/// Максимальная длина строкового поля. Точка данных ограничена по размеру, а
/// причина может прилететь ответом модели на несколько килобайт.
const FIELD_LIMIT: usize = 512;

fn field(body: &serde_json::Value, key: &str) -> String {
    body.get(key)
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .chars()
        .take(FIELD_LIMIT)
        .collect()
}

/// POST /event (app JWT) — записать одну клиентскую ошибку.
///
/// Отвечает 202 и НЕ обещает, что запись случилась: у Analytics Engine запись
/// «выстрелил и забыл», подтверждения не бывает. Клиенту это и не нужно —
/// повторять он не станет.
async fn record_event(mut req: Request, env: &Env, user_id: &str) -> Result<Response> {
    let body: serde_json::Value = req.json().await.unwrap_or(serde_json::json!({}));
    let code = field(&body, "code");
    let kind = field(&body, "kind");
    if code.is_empty() || kind.is_empty() {
        return Ok(error_response("missing code/kind", 400));
    }

    let dataset = match env.analytics_engine(EVENT_DATASET) {
        Ok(d) => d,
        Err(e) => {
            // Биндинга нет — это наша ошибка развёртывания, а не клиента. Громко в
            // лог, но клиенту 202: терять из-за этого его работу незачем.
            console_error!("analytics engine binding {EVENT_DATASET}: {e}");
            return Response::from_json(&serde_json::json!({ "ok": false }))
                .map(|r| r.with_status(202));
        }
    };

    let point = AnalyticsEngineDataPointBuilder::new()
        .indexes([code.as_str()].as_slice())
        .add_blob(kind)
        .add_blob(field(&body, "subject"))
        .add_blob(field(&body, "cause"))
        .add_blob(field(&body, "build"))
        .add_blob(field(&body, "platform"))
        .add_blob(user_id)
        .add_double(1);

    if let Err(e) = point.write_to(&dataset) {
        console_error!("analytics engine write: {e}");
    }
    Response::from_json(&serde_json::json!({ "ok": true })).map(|r| r.with_status(202))
}

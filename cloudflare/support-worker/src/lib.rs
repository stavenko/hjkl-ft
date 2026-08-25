use worker::*;

mod conversation_do;
mod conversation_index_do;
mod curator_index_do;
mod token;
mod types;

pub use conversation_do::ConversationDO;
pub use conversation_index_do::ConversationIndexDO;
pub use curator_index_do::CuratorIndexDO;

use token::validate_from_header;
use types::{AppendResult, ErrorResponse};

const PREVIEW_MAX: usize = 200;
/// Пределы на имена. Имя куратора видит худеющий на экране согласия, имя клиента —
/// только сам куратор; и то и другое — подпись, а не текст, поэтому короткие.
const CURATOR_NAME_MAX: usize = 64;
const CLIENT_NAME_MAX: usize = 64;

// ---- Durable Object stubs ----

fn conversation_stub(env: &Env, user_id: &str) -> Result<worker::durable::Stub> {
    env.durable_object("CONVERSATION_DO")?
        .id_from_name(user_id)?
        .get_stub()
}

fn index_stub(env: &Env) -> Result<worker::durable::Stub> {
    env.durable_object("CONVERSATION_INDEX_DO")?
        .id_from_name("index")?
        .get_stub()
}

fn curator_stub(env: &Env) -> Result<worker::durable::Stub> {
    env.durable_object("CURATOR_INDEX_DO")?
        .id_from_name("curators")?
        .get_stub()
}

/// Build an internal POST request to a Durable Object with the given path and JSON body.
fn do_request(path: &str, body: &serde_json::Value) -> Result<Request> {
    let url = format!("https://internal{path}");
    let body_str = serde_json::to_string(body)
        .map_err(|e| Error::RustError(format!("serialize DO request: {e}")))?;
    Request::new_with_init(
        &url,
        RequestInit::new()
            .with_method(Method::Post)
            .with_body(Some(wasm_bindgen::JsValue::from_str(&body_str))),
    )
}

// ---- error helpers ----

fn json_status(status: u16, message: &str) -> Response {
    let body = ErrorResponse {
        error: message.to_string(),
    };
    Response::from_json(&body)
        .expect("serialize ErrorResponse")
        .with_status(status)
}

// ---- auth gates ----

/// 401 on any signature/format failure; returns the authenticated user_id (sub).
async fn auth_user(req: &Request, env: &Env) -> std::result::Result<String, Response> {
    validate_from_header(req, env)
        .await
        .map_err(|e| json_status(401, &e.to_string()))
}

/// Operator-only secret for POST /admin/approve (X-Admin-Secret). Read like
/// INTERNAL_PUSH_KEY: env.secret first (prod `wrangler secret put`), env.var
/// fallback (dev [vars]). Err means UNSET — the caller MUST fail closed.
async fn admin_approve_secret(env: &Env) -> std::result::Result<String, String> {
    token::secret_or_var(env, "ADMIN_APPROVE_SECRET").await
}

/// 401 if the token is invalid, 403 if a valid sub is not DO-approved. Returns
/// the expert's sub on success.
///
/// The ONLY source of truth is the GLOBAL index DO's `admins` table (runtime-
/// mutable via the approve flow, no redeploy). On ANY DO/stub/parse failure we
/// 500 (fail loudly); there is NO code path that grants expert access without a
/// stored approval.
async fn auth_expert(req: &Request, env: &Env) -> std::result::Result<String, Response> {
    let sub = validate_from_header(req, env)
        .await
        .map_err(|e| json_status(401, &e.to_string()))?;
    let do_req = match do_request("/admin-is-approved", &serde_json::json!({ "sub": sub })) {
        Ok(r) => r,
        Err(e) => return Err(json_status(500, &format!("admin auth: {e}"))),
    };
    let stub = match index_stub(env) {
        Ok(s) => s,
        Err(e) => return Err(json_status(500, &format!("admin auth stub: {e}"))),
    };
    let mut resp = match stub.fetch_with_request(do_req).await {
        Ok(r) => r,
        Err(e) => return Err(json_status(500, &format!("admin auth fetch: {e}"))),
    };
    if resp.status_code() != 200 {
        return Err(json_status(500, "admin auth DO error"));
    }
    let v: serde_json::Value = match resp.json().await {
        Ok(v) => v,
        Err(e) => return Err(json_status(500, &format!("admin auth parse: {e}"))),
    };
    if v.get("approved").and_then(|b| b.as_bool()).unwrap_or(false) {
        Ok(sub)
    } else {
        Err(json_status(403, "not an expert"))
    }
}

/// Позвать кураторский DO и вернуть его JSON. Ошибка стуба/сети/разбора — это
/// 500 и никогда не «нет доступа»: молчаливая деградация в отказ скрыла бы
/// поломку хранилища.
async fn curator_do(
    env: &Env,
    path: &str,
    body: &serde_json::Value,
) -> std::result::Result<serde_json::Value, Response> {
    let do_req = do_request(path, body)
        .map_err(|e| json_status(500, &format!("curator DO request {path}: {e}")))?;
    let stub =
        curator_stub(env).map_err(|e| json_status(500, &format!("curator DO stub: {e}")))?;
    let mut resp = stub
        .fetch_with_request(do_req)
        .await
        .map_err(|e| json_status(500, &format!("curator DO fetch {path}: {e}")))?;
    let status = resp.status_code();
    let v: serde_json::Value = resp
        .json()
        .await
        .map_err(|e| json_status(500, &format!("curator DO parse {path}: {e}")))?;
    if status != 200 {
        let msg = v.get("error").and_then(|e| e.as_str()).unwrap_or("curator DO error");
        return Err(json_status(status, msg));
    }
    Ok(v)
}

/// 401 при негодном токене, 403 если `sub` не заведён куратором.
///
/// В отличие от `auth_expert`, здесь нет одобрения оператором: куратором
/// становится любой, кто позвал `/curator/register`. Гейт проверяет только, что
/// профиль существует, — то есть что человек пришёл из кураторского приложения,
/// а не тычет кураторскими ручками из приложения худеющего.
async fn auth_curator(req: &Request, env: &Env) -> std::result::Result<String, Response> {
    let sub = validate_from_header(req, env)
        .await
        .map_err(|e| json_status(401, &e.to_string()))?;
    let v = curator_do(env, "/curator-get", &serde_json::json!({ "curator_id": sub })).await?;
    if v.get("found").and_then(|b| b.as_bool()).unwrap_or(false) {
        Ok(sub)
    } else {
        Err(json_status(403, "not a curator"))
    }
}

/// Ключ треда «худеющий ↔ собеседник».
///
/// Переписка ЛИЧНАЯ, поэтому тред один на ПАРУ, а не один на человека: новый
/// куратор не должен читать разговор с предыдущим. Пара с админом сохраняет
/// прежний ключ (голый `user_id`) — существующие переписки остаются ровно там,
/// где лежат, и мигрировать нечего.
fn thread_key(user_id: &str, peer: &str) -> String {
    if peer == PEER_ADMIN {
        user_id.to_string()
    } else {
        format!("{user_id}|{peer}")
    }
}

const PEER_ADMIN: &str = "admin";

fn curator_peer(curator_id: &str) -> String {
    format!("curator:{curator_id}")
}

/// Собеседник, заданный клиентом в `?peer=`. Разрешены только две формы; всё
/// прочее отбрасывается, чтобы произвольная строка не стала именем чужого DO.
fn parse_peer(raw: &str) -> Option<String> {
    if raw == PEER_ADMIN {
        return Some(PEER_ADMIN.to_string());
    }
    let id = raw.strip_prefix("curator:")?;
    let ok = !id.is_empty()
        && id.len() <= 128
        && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
    ok.then(|| raw.to_string())
}

/// Кому адресованы сообщения этого худеющего.
pub struct Routing {
    /// Ключ собеседника: `admin` либо `curator:<id>`.
    peer: String,
    /// Куратор и слот, которым он видит человека, — если куратор есть.
    curator: Option<(String, String)>,
}

/// Привязка худеющего. Нет куратора — разговор идёт с админом, как и раньше.
async fn routing_of_user(env: &Env, user_id: &str) -> std::result::Result<Routing, Response> {
    let v = curator_do(env, "/binding", &serde_json::json!({ "user_id": user_id })).await?;
    if !v.get("bound").and_then(|b| b.as_bool()).unwrap_or(false) {
        return Ok(Routing { peer: PEER_ADMIN.to_string(), curator: None });
    }
    let cid = v
        .get("curator_id")
        .and_then(|c| c.as_str())
        .ok_or_else(|| json_status(500, "binding without curator_id"))?;
    let slot = v
        .get("client_id")
        .and_then(|c| c.as_str())
        .ok_or_else(|| json_status(500, "binding without client_id"))?;
    Ok(Routing {
        peer: curator_peer(cid),
        curator: Some((cid.to_string(), slot.to_string())),
    })
}

/// `user_id` клиента куратора. 404, если слот чужой или ещё не привязан — чужой
/// `cid` не должен отличаться от несуществующего.
async fn client_user_id(
    env: &Env,
    curator_id: &str,
    cid: &str,
) -> std::result::Result<String, Response> {
    let v = curator_do(
        env,
        "/client-user",
        &serde_json::json!({ "curator_id": curator_id, "id": cid }),
    )
    .await?;
    v.get("user_id")
        .and_then(|u| u.as_str())
        .map(|s| s.to_string())
        .ok_or_else(|| json_status(404, "client not bound"))
}

fn truncate_preview(text: &str) -> String {
    text.chars().take(PREVIEW_MAX).collect()
}

/// Server-to-server push: nudge `user_id` to re-open the Live chat after an
/// expert reply. Delegates to main-flow's `/push/notify` (the only worker that
/// holds the VAPID keys + the user's push subscriptions), authenticated by the
/// shared `INTERNAL_PUSH_KEY` — the SAME contract payment-worker already uses.
///
/// The deep-link `url` follows the notification convention: `?notif=1` tells the
/// app this navigation came from a tapped push.
///
/// Returns Err on any misconfiguration or non-2xx from main-flow. This is a
/// BEST-EFFORT nudge: the caller (expert_reply) logs the error loudly but does
/// NOT fail the reply on it (the reply is already committed) — matching
/// payment-worker's notifyPush policy.
async fn nudge_user_push(env: &Env, user_id: &str, text: &str) -> Result<()> {
    push_via_main_flow(env, user_id, &truncate_preview(text), "/chat?notif=1").await
}

/// Пуш КУРАТОРУ о новом сообщении клиента. Тот же канал и та же политика
/// «лучшее усилие», что у пуша худеющему: подписки куратора лежат в main-flow
/// под его собственным `sub`, потому что паскей он заводил на своём домене.
async fn nudge_curator_push(env: &Env, curator_id: &str, cid: &str, text: &str) -> Result<()> {
    push_via_main_flow(
        env,
        curator_id,
        &truncate_preview(text),
        &format!("/?notif=1&client={cid}"),
    )
    .await
}

/// Raw push relay: `{userId, body, url}` → main-flow `/push/notify` over the
/// service binding, authenticated with INTERNAL_PUSH_KEY. Shared by the user
/// nudge (relative /chat deep-link) and the admin digest (absolute admin URL).
async fn push_via_main_flow(env: &Env, user_id: &str, text: &str, url: &str) -> Result<()> {
    let key = token::secret_or_var(env, "INTERNAL_PUSH_KEY")
        .await
        .map_err(Error::RustError)?;

    let body = serde_json::json!({
        "userId": user_id,
        "body": text,
        "url": url,
    })
    .to_string();

    let headers = Headers::new();
    headers
        .set("Content-Type", "application/json")
        .map_err(|e| Error::RustError(format!("set header: {e}")))?;
    headers
        .set("X-Internal-Key", &key)
        .map_err(|e| Error::RustError(format!("set header: {e}")))?;

    let mut init = RequestInit::new();
    init.with_method(Method::Post)
        .with_headers(headers)
        .with_body(Some(wasm_bindgen::JsValue::from_str(&body)));

    // The host is irrelevant for a service-binding fetch; only the path routes
    // inside main-flow. Bind-fetch avoids the workers.dev Worker→Worker 404.
    let request = Request::new_with_init("https://main-flow/push/notify", &init)
        .map_err(|e| Error::RustError(format!("build push request: {e}")))?;

    let main_flow = env
        .service("MAIN_FLOW")
        .map_err(|e| Error::RustError(format!("MAIN_FLOW service binding: {e}")))?;
    let resp = main_flow
        .fetch_request(request)
        .await
        .map_err(|e| Error::RustError(format!("push notify fetch failed: {e}")))?;

    let status = resp.status_code();
    if !(200..300).contains(&status) {
        return Err(Error::RustError(format!(
            "push notify returned {status} for user {user_id}"
        )));
    }
    Ok(())
}

/// Hourly admin digest (the cron below; also runnable on demand via
/// POST /admin/digest-run). Counts user messages logged in the IndexDO since the
/// last committed watermark; when non-zero, pushes «Новых сообщений в поддержке: N»
/// to EVERY approved expert (their existing push subscriptions in main-flow) with
/// a deep link to the admin PWA, then commits the watermark = ts of the newest
/// counted message. The very first run has no watermark, so it reports the whole
/// backlog accumulated since this feature deployed. If NO push is delivered the
/// watermark is NOT moved — the next run retries the same window (fail loudly,
/// never lose a digest silently).
async fn run_support_digest(env: &Env) -> Result<serde_json::Value> {
    let peek_req = do_request("/digest-peek", &serde_json::json!({}))?;
    let mut resp = index_stub(env)?.fetch_with_request(peek_req).await?;
    if resp.status_code() != 200 {
        return Err(Error::RustError("support digest: peek failed".into()));
    }
    let v: serde_json::Value = resp.json().await?;
    let count = v.get("count").and_then(|c| c.as_i64()).unwrap_or(0);
    let latest = v
        .get("latest_ts")
        .and_then(|t| t.as_str())
        .unwrap_or("")
        .to_string();
    if count == 0 || latest.is_empty() {
        return Ok(serde_json::json!({ "count": 0, "pushed": 0 }));
    }

    let admins_req = do_request("/admins-list", &serde_json::json!({}))?;
    let mut aresp = index_stub(env)?.fetch_with_request(admins_req).await?;
    let av: serde_json::Value = aresp.json().await?;
    let admins: Vec<String> = av
        .get("admins")
        .and_then(|a| a.as_array())
        .map(|a| a.iter().filter_map(|s| s.as_str().map(String::from)).collect())
        .unwrap_or_default();
    if admins.is_empty() {
        return Err(Error::RustError(
            "support digest: no approved admins to notify".into(),
        ));
    }

    let text = format!("Новых сообщений в поддержке: {count}");
    let mut pushed = 0;
    for sub in &admins {
        match push_via_main_flow(env, sub, &text, "https://admin.renorma.app/?notif=1").await {
            Ok(()) => pushed += 1,
            Err(e) => console_error!("support digest: push to {sub} failed: {e}"),
        }
    }
    if pushed == 0 {
        return Err(Error::RustError("support digest: all pushes failed".into()));
    }

    let commit_req = do_request("/digest-commit", &serde_json::json!({ "ts": latest }))?;
    let cresp = index_stub(env)?.fetch_with_request(commit_req).await?;
    if cresp.status_code() != 200 {
        return Err(Error::RustError("support digest: commit failed".into()));
    }
    Ok(serde_json::json!({ "count": count, "pushed": pushed }))
}

/// Expert-only: run the digest immediately — same code path as the hourly cron.
async fn admin_digest_run(req: Request, ctx: RouteContext<()>) -> Result<Response> {
    if let Err(resp) = auth_expert(&req, &ctx.env).await {
        return Ok(resp);
    }
    let v = run_support_digest(&ctx.env).await?;
    Response::from_json(&v)
}

/// Parse an AppendResult from a DO response.
async fn read_append(resp: &mut Response) -> Result<AppendResult> {
    if resp.status_code() != 200 {
        return Err(Error::RustError(format!(
            "conversation DO append failed: {}",
            resp.status_code()
        )));
    }
    resp.json().await
}

// ---- USER handlers ----

async fn user_send(mut req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let uid = match auth_user(&req, &ctx.env).await {
        Ok(s) => s,
        Err(resp) => return Ok(resp),
    };

    let body: serde_json::Value = req.json().await?;
    let client_id = body.get("client_id").and_then(|v| v.as_str()).unwrap_or("");
    let text = body.get("text").and_then(|v| v.as_str()).unwrap_or("");
    if client_id.is_empty() || text.is_empty() {
        return Ok(json_status(400, "client_id and text are required"));
    }
    // Typed envelope (default kind='text'). `payload` is a RAW JSON string;
    // forwarded verbatim to the DO. See the data-request/data-share protocol.
    let (kind, payload) = typed_envelope(&body);

    // Кому адресовано: есть куратор — ему, нет — админу. Развилка здесь и только
    // здесь, поэтому приложение худеющего про адресата ничего знать не обязано.
    let routing = match routing_of_user(&ctx.env, &uid).await {
        Ok(r) => r,
        Err(resp) => return Ok(resp),
    };

    let append_req = do_request(
        "/append",
        &serde_json::json!({
            "client_id": client_id,
            "text": text,
            "sender": "user",
            "kind": kind,
            "payload": payload,
        }),
    )?;
    let mut do_resp = conversation_stub(&ctx.env, &thread_key(&uid, &routing.peer))?
        .fetch_with_request(append_req)
        .await?;
    let result = read_append(&mut do_resp).await?;

    match &routing.curator {
        // Куратора нет — разговор с админом, и очередь админа ведётся как прежде.
        None => {
            // Index maintenance — ALWAYS call it, even on a deduped (retried) append.
            // touch-user is idempotent + monotonic (a deduped/older seq is a no-op), so a
            // retry self-heals a previously-failed index touch instead of corrupting the
            // queue. We still fail loudly on a genuine index error so the client retries.
            let touch_req = do_request(
                "/touch-user",
                &serde_json::json!({
                    "user_id": uid,
                    "preview": truncate_preview(text),
                    "last_ts": result.created_at,
                    "last_seq": result.seq,
                }),
            )?;
            let touch_resp = index_stub(&ctx.env)?.fetch_with_request(touch_req).await?;
            if touch_resp.status_code() != 200 {
                return Err(Error::RustError("index touch-user failed".into()));
            }
        }
        // Есть куратор — в очередь админа это НЕ попадает: админ видит только то,
        // что написали ему. Куратора будим пушем, лучшим усилием: сообщение уже
        // записано, и падение пуша не имеет права его отменить.
        Some((cid, slot)) => {
            // Отчёт кладётся в слот и гасит запрос. Это НЕ второе хранилище
            // данных человека, а снимок последнего отчёта: без него дашборд
            // куратора перелистывал бы переписку на каждом открытии.
            if kind == "data_share" {
                if let Some(payload) = payload.as_deref() {
                    if let Err(resp) = curator_do(
                        &ctx.env,
                        "/report-put",
                        &serde_json::json!({ "user_id": uid, "payload": payload }),
                    )
                    .await
                    {
                        return Ok(resp);
                    }
                }
            }
            if let Err(e) = nudge_curator_push(&ctx.env, cid, slot, text).await {
                console_error!("user_send push nudge to curator {cid} failed: {e}");
            }
        }
    }

    // peer в ответе — чтобы приложение положило отправленное в тот же тред, куда
    // его положил сервер. Развилку «куратор или админ» решает он, и угадывать её
    // на клиенте значит однажды разойтись.
    Response::from_json(&serde_json::json!({
        "seq": result.seq,
        "created_at": result.created_at,
        "peer": routing.peer,
    }))
}


/// Приложение опрашивает ТЕКУЩИЙ тред: архивные не меняются, и их история
/// приезжает синком. `?peer=` — страховка для устройства, которое ещё не
/// досинкалось и хочет дочитать конкретную переписку с сервера.
async fn user_messages(req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let uid = match auth_user(&req, &ctx.env).await {
        Ok(s) => s,
        Err(resp) => return Ok(resp),
    };
    let url = req.url()?;
    let q: std::collections::HashMap<_, _> = url.query_pairs().into_owned().collect();
    let peer = match q.get("peer") {
        Some(raw) => match parse_peer(raw) {
            Some(p) => p,
            None => return Ok(json_status(400, "bad peer")),
        },
        None => match routing_of_user(&ctx.env, &uid).await {
            Ok(r) => r.peer,
            Err(resp) => return Ok(resp),
        },
    };
    let (after_seq, limit) = parse_paging(&req)?;
    let wait_ms = parse_wait_ms(&req);
    let list_req = do_request(
        "/list",
        &serde_json::json!({ "after_seq": after_seq, "limit": limit, "wait_ms": wait_ms }),
    )?;
    let mut resp = conversation_stub(&ctx.env, &thread_key(&uid, &peer))?
        .fetch_with_request(list_req)
        .await?;
    // Отдаём собеседника вместе со страницей: клиент раскладывает кэш по
    // собеседникам и без этого не знал бы, куда лёг ответ на «текущий тред».
    let mut v: serde_json::Value = resp.json().await?;
    v["peer"] = serde_json::Value::String(peer);
    Response::from_json(&v)
}

async fn user_read(mut req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let uid = match auth_user(&req, &ctx.env).await {
        Ok(s) => s,
        Err(resp) => return Ok(resp),
    };
    let body: serde_json::Value = req.json().await?;
    let seq = body
        .get("seq")
        .and_then(|v| v.as_i64())
        .ok_or_else(|| Error::RustError("missing seq".into()))?;
    let peer = match routing_of_user(&ctx.env, &uid).await {
        Ok(r) => r.peer,
        Err(resp) => return Ok(resp),
    };
    let read_req = do_request("/read", &serde_json::json!({ "who": "user", "seq": seq }))?;
    conversation_stub(&ctx.env, &thread_key(&uid, &peer))?
        .fetch_with_request(read_req)
        .await
}

// ---- EXPERT handlers ----

async fn expert_conversations(req: Request, ctx: RouteContext<()>) -> Result<Response> {
    if let Err(resp) = auth_expert(&req, &ctx.env).await {
        return Ok(resp);
    }
    let url = req.url()?;
    let q: std::collections::HashMap<_, _> = url.query_pairs().into_owned().collect();
    let status = q.get("status").map(|s| s.as_str()).unwrap_or("pending");
    let after = q.get("after").map(|s| s.as_str());
    let limit: i64 = q.get("limit").and_then(|s| s.parse().ok()).unwrap_or(50);

    let mut body = serde_json::json!({ "status": status, "limit": limit });
    if let Some(a) = after {
        body["after"] = serde_json::json!(a);
    }
    let req = do_request("/conversations", &body)?;
    index_stub(&ctx.env)?.fetch_with_request(req).await
}

async fn expert_messages(req: Request, ctx: RouteContext<()>) -> Result<Response> {
    if let Err(resp) = auth_expert(&req, &ctx.env).await {
        return Ok(resp);
    }
    let uid = ctx
        .param("uid")
        .ok_or_else(|| Error::RustError("missing uid".into()))?
        .clone();
    let (after_seq, limit) = parse_paging(&req)?;
    let wait_ms = parse_wait_ms(&req);
    let list_req = do_request(
        "/list",
        &serde_json::json!({ "after_seq": after_seq, "limit": limit, "wait_ms": wait_ms }),
    )?;
    conversation_stub(&ctx.env, &uid)?
        .fetch_with_request(list_req)
        .await
}

async fn expert_reply(mut req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let expert_sub = match auth_expert(&req, &ctx.env).await {
        Ok(s) => s,
        Err(resp) => return Ok(resp),
    };
    let uid = ctx
        .param("uid")
        .ok_or_else(|| Error::RustError("missing uid".into()))?
        .clone();

    let body: serde_json::Value = req.json().await?;
    let client_id = body.get("client_id").and_then(|v| v.as_str()).unwrap_or("");
    let text = body.get("text").and_then(|v| v.as_str()).unwrap_or("");
    if client_id.is_empty() || text.is_empty() {
        return Ok(json_status(400, "client_id and text are required"));
    }
    // Typed envelope (default kind='text'). A curator data-request rides here as
    // kind='data_request' with the {dataset} payload; forwarded verbatim.
    let (kind, payload) = typed_envelope(&body);

    let append_req = do_request(
        "/append",
        &serde_json::json!({
            "client_id": client_id,
            "text": text,
            "sender": "expert",
            "expert_id": expert_sub,
            "kind": kind,
            "payload": payload,
        }),
    )?;
    let mut do_resp = conversation_stub(&ctx.env, &uid)?
        .fetch_with_request(append_req)
        .await?;
    let result = read_append(&mut do_resp).await?;

    // Index maintenance — ALWAYS call it, even on a deduped (retried) reply.
    // clear-pending is idempotent + monotonic: it clears pending ONLY IF no newer
    // user message arrived after this reply's seq (existing last_seq <= reply_seq).
    // So a retried/stale reply can't drop a conversation the user re-opened, and
    // a previously-failed clear self-heals on retry. `last_seq` carries THIS reply's
    // seq, used by the DO as reply_seq.
    let clear_req = do_request(
        "/clear-pending",
        &serde_json::json!({
            "user_id": uid,
            "preview": truncate_preview(text),
            "last_ts": result.created_at,
            "last_seq": result.seq,
        }),
    )?;
    let clear_resp = index_stub(&ctx.env)?.fetch_with_request(clear_req).await?;
    if clear_resp.status_code() != 200 {
        return Err(Error::RustError("index clear-pending failed".into()));
    }

    // Nudge the user to re-open the Live chat. BEST-EFFORT (same contract +
    // policy as payment-worker's notifyPush): the reply is already committed +
    // indexed above, so a push failure MUST NOT fail the reply — it is logged
    // loudly (never swallowed silently) and the request still succeeds.
    if let Err(e) = nudge_user_push(&ctx.env, &uid, text).await {
        console_error!("expert_reply push nudge failed for user {uid}: {e}");
    }

    Response::from_json(&serde_json::json!({ "seq": result.seq }))
}

async fn expert_read(mut req: Request, ctx: RouteContext<()>) -> Result<Response> {
    if let Err(resp) = auth_expert(&req, &ctx.env).await {
        return Ok(resp);
    }
    let uid = ctx
        .param("uid")
        .ok_or_else(|| Error::RustError("missing uid".into()))?
        .clone();
    let body: serde_json::Value = req.json().await?;
    let seq = body
        .get("seq")
        .and_then(|v| v.as_i64())
        .ok_or_else(|| Error::RustError("missing seq".into()))?;
    let read_req = do_request("/read", &serde_json::json!({ "who": "expert", "seq": seq }))?;
    conversation_stub(&ctx.env, &uid)?
        .fetch_with_request(read_req)
        .await
}

// ---- CURATOR handlers ----
//
// Куратор — свободная роль: регистрируется сам, никого не спрашивая. Всё, что
// ниже, кроме /curator/register, требует уже заведённого профиля (`auth_curator`),
// и КАЖДАЯ операция над клиентом привязана к `curator_id` вызывающего — чужой
// `cid` не находится и отвечает 404, а не чужими данными.

/// POST /curator/register (JWT). Идемпотентно: повторный вызов возвращает
/// заведённый профиль. Паскей человек создал на кураторском домене — здесь
/// заводится только профиль под его `sub`.
async fn curator_register(req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let sub = match auth_user(&req, &ctx.env).await {
        Ok(s) => s,
        Err(resp) => return Ok(resp),
    };
    match curator_do(
        &ctx.env,
        "/curator-register",
        &serde_json::json!({ "curator_id": sub }),
    )
    .await
    {
        Ok(v) => Response::from_json(&v),
        Err(resp) => Ok(resp),
    }
}

/// GET /curator/me — профиль (имя и язык).
async fn curator_me(req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let sub = match auth_curator(&req, &ctx.env).await {
        Ok(s) => s,
        Err(resp) => return Ok(resp),
    };
    match curator_do(&ctx.env, "/curator-get", &serde_json::json!({ "curator_id": sub })).await {
        Ok(v) => Response::from_json(&v),
        Err(resp) => Ok(resp),
    }
}

/// POST /curator/me {name?, lang?} — правка своего профиля.
async fn curator_me_set(mut req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let sub = match auth_curator(&req, &ctx.env).await {
        Ok(s) => s,
        Err(resp) => return Ok(resp),
    };
    let body: serde_json::Value = req.json().await.unwrap_or(serde_json::Value::Null);
    let mut call = serde_json::json!({ "curator_id": sub });
    if let Some(name) = body.get("name").and_then(|v| v.as_str()) {
        let name = name.trim();
        if name.chars().count() > CURATOR_NAME_MAX {
            return Ok(json_status(400, "name too long"));
        }
        call["name"] = serde_json::Value::String(name.to_string());
    }
    if let Some(lang) = body.get("lang").and_then(|v| v.as_str()) {
        if !matches!(lang, "ru" | "en") {
            return Ok(json_status(400, "unsupported lang"));
        }
        call["lang"] = serde_json::Value::String(lang.to_string());
    }
    match curator_do(&ctx.env, "/curator-set", &call).await {
        Ok(v) => Response::from_json(&v),
        Err(resp) => Ok(resp),
    }
}

/// GET /curator/clients — список слотов.
async fn curator_clients(req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let sub = match auth_curator(&req, &ctx.env).await {
        Ok(s) => s,
        Err(resp) => return Ok(resp),
    };
    match curator_do(&ctx.env, "/client-list", &serde_json::json!({ "curator_id": sub })).await {
        Ok(v) => Response::from_json(&v),
        Err(resp) => Ok(resp),
    }
}

/// POST /curator/clients {name} — завести слот и получить пригласительный код.
async fn curator_client_create(mut req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let sub = match auth_curator(&req, &ctx.env).await {
        Ok(s) => s,
        Err(resp) => return Ok(resp),
    };
    let body: serde_json::Value = req.json().await.unwrap_or(serde_json::Value::Null);
    let name = body.get("name").and_then(|v| v.as_str()).unwrap_or("").trim();
    if name.is_empty() {
        return Ok(json_status(400, "name required"));
    }
    if name.chars().count() > CLIENT_NAME_MAX {
        return Ok(json_status(400, "name too long"));
    }
    match curator_do(
        &ctx.env,
        "/client-create",
        &serde_json::json!({ "curator_id": sub, "name": name }),
    )
    .await
    {
        Ok(v) => Response::from_json(&v),
        Err(resp) => Ok(resp),
    }
}

/// POST /curator/clients/:cid/rename {name}. POST, а не PATCH: CORS-преамбула
/// воркера разрешает только GET/POST/OPTIONS, и заводить ради переименования
/// новый метод — лишний повод для сюрприза в браузере.
async fn curator_client_rename(mut req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let sub = match auth_curator(&req, &ctx.env).await {
        Ok(s) => s,
        Err(resp) => return Ok(resp),
    };
    let Some(cid) = ctx.param("cid").map(|s| s.to_string()) else {
        return Ok(json_status(400, "missing cid"));
    };
    let body: serde_json::Value = req.json().await.unwrap_or(serde_json::Value::Null);
    let name = body.get("name").and_then(|v| v.as_str()).unwrap_or("").trim();
    if name.is_empty() {
        return Ok(json_status(400, "name required"));
    }
    if name.chars().count() > CLIENT_NAME_MAX {
        return Ok(json_status(400, "name too long"));
    }
    match curator_do(
        &ctx.env,
        "/client-rename",
        &serde_json::json!({ "curator_id": sub, "id": cid, "name": name }),
    )
    .await
    {
        Ok(v) => Response::from_json(&v),
        Err(resp) => Ok(resp),
    }
}

/// POST /curator/clients/:cid/delete — убрать слот из списка совсем.
async fn curator_client_delete(req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let sub = match auth_curator(&req, &ctx.env).await {
        Ok(s) => s,
        Err(resp) => return Ok(resp),
    };
    let Some(cid) = ctx.param("cid").map(|s| s.to_string()) else {
        return Ok(json_status(400, "missing cid"));
    };
    match curator_do(
        &ctx.env,
        "/client-delete",
        &serde_json::json!({ "curator_id": sub, "id": cid }),
    )
    .await
    {
        Ok(v) => Response::from_json(&v),
        Err(resp) => Ok(resp),
    }
}

/// GET /curator/invite/:code (JWT худеющего). Что показать на экране согласия.
///
/// Гейт — обычный пользовательский токен: приглашение открывает ЧЕЛОВЕК, а не
/// куратор. Код гасится согласием, а не открытием, поэтому неоткрытую ссылку
/// можно переслать ещё раз.
async fn curator_invite_peek(req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let sub = match auth_user(&req, &ctx.env).await {
        Ok(s) => s,
        Err(resp) => return Ok(resp),
    };
    let Some(code) = ctx.param("code").map(|s| s.to_string()) else {
        return Ok(json_status(400, "missing code"));
    };
    match curator_do(
        &ctx.env,
        "/invite-peek",
        &serde_json::json!({ "code": code, "user_id": sub }),
    )
    .await
    {
        Ok(v) => Response::from_json(&v),
        Err(resp) => Ok(resp),
    }
}

/// POST /curator/invite/:code/accept (JWT худеющего) — согласие.
///
/// Привязка ставится на `sub` ИЗ ТОКЕНА, никогда из тела: иначе приглашением
/// можно было бы привязать чужой аккаунт.
async fn curator_invite_accept(req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let sub = match auth_user(&req, &ctx.env).await {
        Ok(s) => s,
        Err(resp) => return Ok(resp),
    };
    let Some(code) = ctx.param("code").map(|s| s.to_string()) else {
        return Ok(json_status(400, "missing code"));
    };
    match curator_do(
        &ctx.env,
        "/invite-accept",
        &serde_json::json!({ "code": code, "user_id": sub }),
    )
    .await
    {
        Ok(v) => Response::from_json(&v),
        Err(resp) => Ok(resp),
    }
}

/// GET /curator/binding (JWT худеющего) — есть ли у меня куратор и как его зовут.
/// Этим же ответом приложение решает, показывать ли виджет отчёта.
async fn curator_binding(req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let sub = match auth_user(&req, &ctx.env).await {
        Ok(s) => s,
        Err(resp) => return Ok(resp),
    };
    match curator_do(&ctx.env, "/binding", &serde_json::json!({ "user_id": sub })).await {
        Ok(v) => Response::from_json(&v),
        Err(resp) => Ok(resp),
    }
}

/// POST /curator/unbind (JWT худеющего) — «больше не хочу куратора».
async fn curator_unbind_by_user(req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let sub = match auth_user(&req, &ctx.env).await {
        Ok(s) => s,
        Err(resp) => return Ok(resp),
    };
    match curator_do(&ctx.env, "/unbind", &serde_json::json!({ "user_id": sub })).await {
        Ok(v) => Response::from_json(&v),
        Err(resp) => Ok(resp),
    }
}

/// POST /curator/clients/:cid/unbind (куратор) — «прекращаю работу с этим человеком».
/// Слот остаётся в списке с новым кодом: тем же слотом человека приглашают снова.
async fn curator_client_unbind(req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let sub = match auth_curator(&req, &ctx.env).await {
        Ok(s) => s,
        Err(resp) => return Ok(resp),
    };
    let Some(cid) = ctx.param("cid").map(|s| s.to_string()) else {
        return Ok(json_status(400, "missing cid"));
    };
    match curator_do(
        &ctx.env,
        "/unbind",
        &serde_json::json!({ "curator_id": sub, "id": cid }),
    )
    .await
    {
        Ok(v) => Response::from_json(&v),
        Err(resp) => Ok(resp),
    }
}

/// Имя куратора для подписи под его сообщениями. Пустое — не беда: подпись
/// пропадёт, а сообщение дойдёт.
async fn curator_name(env: &Env, curator_id: &str) -> Option<String> {
    let v = curator_do(env, "/curator-get", &serde_json::json!({ "curator_id": curator_id }))
        .await
        .ok()?;
    v.get("curator")
        .and_then(|c| c.get("name"))
        .and_then(|n| n.as_str())
        .filter(|n| !n.is_empty())
        .map(|n| n.to_string())
}

/// GET /curator/clients/:cid/messages — тот же длинный опрос, что у эксперта, но
/// только по СВОЕМУ клиенту.
async fn curator_messages(req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let sub = match auth_curator(&req, &ctx.env).await {
        Ok(s) => s,
        Err(resp) => return Ok(resp),
    };
    let Some(cid) = ctx.param("cid").map(|s| s.to_string()) else {
        return Ok(json_status(400, "missing cid"));
    };
    let uid = match client_user_id(&ctx.env, &sub, &cid).await {
        Ok(u) => u,
        Err(resp) => return Ok(resp),
    };
    let (after_seq, limit) = parse_paging(&req)?;
    let wait_ms = parse_wait_ms(&req);
    let list_req = do_request(
        "/list",
        &serde_json::json!({ "after_seq": after_seq, "limit": limit, "wait_ms": wait_ms }),
    )?;
    conversation_stub(&ctx.env, &thread_key(&uid, &curator_peer(&sub)))?
        .fetch_with_request(list_req)
        .await
}

/// POST /curator/clients/:cid/reply — ответ куратора, с подписью его именем.
async fn curator_reply(mut req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let sub = match auth_curator(&req, &ctx.env).await {
        Ok(s) => s,
        Err(resp) => return Ok(resp),
    };
    let Some(cid) = ctx.param("cid").map(|s| s.to_string()) else {
        return Ok(json_status(400, "missing cid"));
    };
    let uid = match client_user_id(&ctx.env, &sub, &cid).await {
        Ok(u) => u,
        Err(resp) => return Ok(resp),
    };
    let body: serde_json::Value = req.json().await?;
    let client_id = body.get("client_id").and_then(|v| v.as_str()).unwrap_or("");
    let text = body.get("text").and_then(|v| v.as_str()).unwrap_or("");
    if client_id.is_empty() || text.is_empty() {
        return Ok(json_status(400, "client_id and text are required"));
    }
    let (kind, payload) = typed_envelope(&body);
    let name = curator_name(&ctx.env, &sub).await;

    let append_req = do_request(
        "/append",
        &serde_json::json!({
            "client_id": client_id,
            "text": text,
            "sender": "expert",
            "expert_id": sub,
            "kind": kind,
            "payload": payload,
            "sender_name": name,
        }),
    )?;
    let mut do_resp = conversation_stub(&ctx.env, &thread_key(&uid, &curator_peer(&sub)))?
        .fetch_with_request(append_req)
        .await?;
    let result = read_append(&mut do_resp).await?;

    // Очередь админа тут ни при чём — это не его переписка. Единственное
    // последействие: разбудить человека, лучшим усилием (ответ уже записан).
    if let Err(e) = nudge_user_push(&ctx.env, &uid, text).await {
        console_error!("curator_reply push nudge failed for user {uid}: {e}");
    }

    Response::from_json(&serde_json::json!({ "seq": result.seq }))
}

/// POST /curator/clients/:cid/read — отметка прочтения на стороне куратора.
async fn curator_read(mut req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let sub = match auth_curator(&req, &ctx.env).await {
        Ok(s) => s,
        Err(resp) => return Ok(resp),
    };
    let Some(cid) = ctx.param("cid").map(|s| s.to_string()) else {
        return Ok(json_status(400, "missing cid"));
    };
    let uid = match client_user_id(&ctx.env, &sub, &cid).await {
        Ok(u) => u,
        Err(resp) => return Ok(resp),
    };
    let body: serde_json::Value = req.json().await?;
    let seq = body
        .get("seq")
        .and_then(|v| v.as_i64())
        .ok_or_else(|| Error::RustError("missing seq".into()))?;
    let read_req = do_request("/read", &serde_json::json!({ "who": "expert", "seq": seq }))?;
    conversation_stub(&ctx.env, &thread_key(&uid, &curator_peer(&sub)))?
        .fetch_with_request(read_req)
        .await
}

/// Предел на период запроса. Куратор пишет число дней свободно, но отчёт
/// собирается на устройстве человека, и «за всё время» — это не запрос, а
/// выгрузка. Год покрывает любую осмысленную работу.
const REQUEST_DAYS_MAX: i64 = 366;

/// POST /curator/clients/:cid/request {days} — «пришлите данные за N дней».
///
/// Запрос — это СООБЩЕНИЕ в треде (kind=data_request), а не флаг на сервере:
/// приложение худеющего и так читает тред, и из него же считает состояние
/// виджета. Отметка в слоте нужна куратору, чтобы видеть, что он уже просил.
async fn curator_request_data(mut req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let sub = match auth_curator(&req, &ctx.env).await {
        Ok(s) => s,
        Err(resp) => return Ok(resp),
    };
    let Some(cid) = ctx.param("cid").map(|s| s.to_string()) else {
        return Ok(json_status(400, "missing cid"));
    };
    let uid = match client_user_id(&ctx.env, &sub, &cid).await {
        Ok(u) => u,
        Err(resp) => return Ok(resp),
    };
    let body: serde_json::Value = req.json().await?;
    let days = body.get("days").and_then(|v| v.as_i64()).unwrap_or(1);
    if days < 1 || days > REQUEST_DAYS_MAX {
        return Ok(json_status(400, "days out of range"));
    }
    let client_id = body.get("client_id").and_then(|v| v.as_str()).unwrap_or("");
    if client_id.is_empty() {
        return Ok(json_status(400, "client_id required"));
    }
    let name = curator_name(&ctx.env, &sub).await;

    // text — запасной вариант для старых сборок приложения; свежие собирают
    // текст сами из kind+payload на языке ЧЕЛОВЕКА, а не куратора.
    let append_req = do_request(
        "/append",
        &serde_json::json!({
            "client_id": client_id,
            "text": "Куратор запрашивает у вас данные",
            "sender": "expert",
            "expert_id": sub,
            "kind": "data_request",
            "payload": serde_json::json!({ "days": days }).to_string(),
            "sender_name": name,
        }),
    )?;
    let mut do_resp = conversation_stub(&ctx.env, &thread_key(&uid, &curator_peer(&sub)))?
        .fetch_with_request(append_req)
        .await?;
    let result = read_append(&mut do_resp).await?;

    if let Err(resp) = curator_do(
        &ctx.env,
        "/request-set",
        &serde_json::json!({ "curator_id": sub, "id": cid, "days": days }),
    )
    .await
    {
        return Ok(resp);
    }

    // Пуш «куратор запросил данные» — один на запрос, без напоминаний. Тело
    // по-русски: main-flow не знает языка человека, а заводить ради этого
    // передачу локали — отдельная работа, которой здесь не место.
    let push_text = match &name {
        Some(n) => format!("{n} запрашивает ваши данные"),
        None => "Куратор запрашивает ваши данные".to_string(),
    };
    if let Err(e) =
        push_via_main_flow(&ctx.env, &uid, &push_text, "/?notif=1&report=1").await
    {
        console_error!("curator_request_data push failed for user {uid}: {e}");
    }

    Response::from_json(&serde_json::json!({ "seq": result.seq }))
}

/// GET /curator/clients/:cid/report — последний присланный отчёт и состояние
/// открытого запроса.
async fn curator_report(req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let sub = match auth_curator(&req, &ctx.env).await {
        Ok(s) => s,
        Err(resp) => return Ok(resp),
    };
    let Some(cid) = ctx.param("cid").map(|s| s.to_string()) else {
        return Ok(json_status(400, "missing cid"));
    };
    match curator_do(
        &ctx.env,
        "/report-get",
        &serde_json::json!({ "curator_id": sub, "id": cid }),
    )
    .await
    {
        Ok(v) => Response::from_json(&v),
        Err(resp) => Ok(resp),
    }
}

// ---- ADMIN authorization handlers ----

/// GET /admin/me (user JWT). Returns the DO's {"approved":bool,"code":string|null}
/// verbatim so the admin UI knows whether to show the queue or request-access.
async fn admin_me(req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let sub = match auth_user(&req, &ctx.env).await {
        Ok(s) => s,
        Err(resp) => return Ok(resp),
    };
    let do_req = do_request("/admin-get", &serde_json::json!({ "sub": sub }))?;
    index_stub(&ctx.env)?.fetch_with_request(do_req).await
}

/// POST /admin/request (user JWT). The code maps to THIS token's authenticated
/// sub — never a body field (INVARIANT 3). Idempotent: returns the same code.
async fn admin_request(req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let sub = match auth_user(&req, &ctx.env).await {
        Ok(s) => s,
        Err(resp) => return Ok(resp),
    };
    let do_req = do_request("/admin-request", &serde_json::json!({ "sub": sub }))?;
    index_stub(&ctx.env)?.fetch_with_request(do_req).await
}

/// POST /admin/approve (X-Admin-Secret header, NO user JWT). Requires the header
/// to equal ADMIN_APPROVE_SECRET; an unset/empty secret fails closed (never
/// approve-anyone, INVARIANT 1). Only {code} is sent to the DO; the approved sub
/// is resolved from STORAGE there, never from this caller (INVARIANT 2).
async fn admin_approve(mut req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let secret = match admin_approve_secret(&ctx.env).await {
        Ok(s) => s,
        Err(e) => return Ok(json_status(500, &e)),
    };
    let provided = req
        .headers()
        .get("X-Admin-Secret")
        .ok()
        .flatten()
        .unwrap_or_default();
    if provided.is_empty() || provided != secret {
        return Ok(json_status(403, "bad admin secret"));
    }
    let body: serde_json::Value = req.json().await?;
    let code = body.get("code").and_then(|v| v.as_str()).unwrap_or("");
    if code.is_empty() {
        return Ok(json_status(400, "code is required"));
    }
    let do_req = do_request("/admin-approve", &serde_json::json!({ "code": code }))?;
    index_stub(&ctx.env)?.fetch_with_request(do_req).await
}

/// POST /internal/is-admin {sub} -> {approved}. Cross-worker admin check, called
/// by payment-worker's require_admin via the SUPPORT_WORKER service binding. SAME
/// source of truth as auth_expert: the DO `admins` table — one approved-admins
/// store, no redeploy to add an admin. Guarded by the
/// shared INTERNAL_PUSH_KEY (X-Internal-Key); an unset key fails closed (500), a
/// wrong/missing key 403s. NEVER swallows: any DO/stub/parse error 500s.
/// Destructive surface, reachable ONLY through a service binding: a binding fetch
/// carries the dummy host the caller dialled (`https://support-worker/…`), which no
/// request off the internet can produce. Host + shared key are BOTH required; the
/// caller has already proven the operator is an approved admin.
const INTERNAL_HOST: &str = "support-worker";

async fn require_binding_internal(
    req: &Request,
    ctx: &RouteContext<()>,
) -> std::result::Result<(), Response> {
    let host = req.url().ok().and_then(|u| u.host_str().map(str::to_string));
    if host.as_deref() != Some(INTERNAL_HOST) {
        return Err(Response::error("Not found", 404).unwrap());
    }
    let key = match token::secret_or_var(&ctx.env, "INTERNAL_PUSH_KEY").await {
        Ok(k) if !k.is_empty() => k,
        Ok(_) => return Err(json_status(500, "internal key not configured")),
        Err(e) => return Err(json_status(500, &e)),
    };
    let provided = req.headers().get("X-Internal-Key").ok().flatten().unwrap_or_default();
    if provided != key {
        return Err(json_status(403, "bad internal key"));
    }
    Ok(())
}

/// POST /internal/user-wipe {userId} — erase the support thread and drop the user
/// from the operator queue/index. Errors are returned, never swallowed.
async fn internal_user_wipe(mut req: Request, ctx: RouteContext<()>) -> Result<Response> {
    if let Err(resp) = require_binding_internal(&req, &ctx).await {
        return Ok(resp);
    }
    let body: serde_json::Value = req.json().await?;
    let user_id = body.get("userId").and_then(|v| v.as_str()).unwrap_or("");
    if user_id.is_empty() {
        return Ok(json_status(400, "userId required"));
    }

    // Тред с куратора стираем ДО отвязки: после неё узнать, с кем человек
    // переписывался, будет уже не по чему.
    let curator_thread = match routing_of_user(&ctx.env, user_id).await {
        Ok(r) => r.curator.map(|(cid, _)| thread_key(user_id, &curator_peer(&cid))),
        Err(resp) => return Ok(resp),
    };
    if let Some(key) = curator_thread {
        let do_req = do_request("/wipe", &serde_json::json!({}))?;
        let resp = conversation_stub(&ctx.env, &key)?.fetch_with_request(do_req).await?;
        if resp.status_code() != 200 {
            return Ok(json_status(502, "curator conversation wipe failed"));
        }
    }

    let do_req = do_request("/wipe", &serde_json::json!({}))?;
    let resp = conversation_stub(&ctx.env, user_id)?.fetch_with_request(do_req).await?;
    if resp.status_code() != 200 {
        return Ok(json_status(502, "conversation wipe failed"));
    }
    let do_req = do_request("/forget-user", &serde_json::json!({ "user_id": user_id }))?;
    let resp = index_stub(&ctx.env)?.fetch_with_request(do_req).await?;
    if resp.status_code() != 200 {
        return Ok(json_status(502, "conversation index forget failed"));
    }
    // Привязка и кэш отчёта — тоже след человека, и уходят вместе с ним. Слот
    // остаётся у куратора: это его запись, а не данные худеющего.
    if let Err(resp) =
        curator_do(&ctx.env, "/forget-user", &serde_json::json!({ "user_id": user_id })).await
    {
        return Ok(resp);
    }
    console_log!("support: wiped conversation for {user_id}");
    Response::from_json(&serde_json::json!({ "ok": true }))
}

async fn internal_is_admin(mut req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let key = match token::secret_or_var(&ctx.env, "INTERNAL_PUSH_KEY").await {
        Ok(k) => k,
        Err(e) => return Ok(json_status(500, &e)),
    };
    let provided = req
        .headers()
        .get("X-Internal-Key")
        .ok()
        .flatten()
        .unwrap_or_default();
    if provided.is_empty() || provided != key {
        return Ok(json_status(403, "bad internal key"));
    }

    let body: serde_json::Value = req.json().await?;
    let sub = body.get("sub").and_then(|v| v.as_str()).unwrap_or("");
    if sub.is_empty() {
        return Ok(json_status(400, "sub required"));
    }

    // SAME logic as auth_expert: the DO admins table.
    let do_req = do_request("/admin-is-approved", &serde_json::json!({ "sub": sub }))?;
    let mut resp = index_stub(&ctx.env)?.fetch_with_request(do_req).await?;
    if resp.status_code() != 200 {
        return Ok(json_status(500, "admin auth DO error"));
    }
    let v: serde_json::Value = resp.json().await?;
    let approved = v.get("approved").and_then(|b| b.as_bool()).unwrap_or(false);
    Response::from_json(&serde_json::json!({ "approved": approved }))
}

/// Extract the typed-envelope fields from a message request body.
///
/// `kind` defaults to "text". `payload` is normalised to a RAW JSON STRING (the
/// storage/read contract): a JSON string passes through verbatim, a JSON object
/// is stringified, anything else (or absent) becomes None. This is forwarded to
/// the DO's `/append` and stored/returned unchanged.
fn typed_envelope(body: &serde_json::Value) -> (String, Option<String>) {
    let kind = body
        .get("kind")
        .and_then(|v| v.as_str())
        .unwrap_or("text")
        .to_string();
    let payload = match body.get("payload") {
        Some(serde_json::Value::String(s)) => Some(s.clone()),
        Some(serde_json::Value::Null) | None => None,
        Some(other) => Some(other.to_string()),
    };
    (kind, payload)
}

fn parse_paging(req: &Request) -> Result<(i64, i64)> {
    let url = req.url()?;
    let q: std::collections::HashMap<_, _> = url.query_pairs().into_owned().collect();
    let after_seq: i64 = q.get("after_seq").and_then(|s| s.parse().ok()).unwrap_or(0);
    let limit: i64 = q.get("limit").and_then(|s| s.parse().ok()).unwrap_or(50);
    Ok((after_seq, limit))
}

/// `?wait=<seconds>` → hold the /list open for up to that long (long-poll).
/// Absent/0 = immediate one-shot read. Clamped to [0, 25] s (the DO caps too).
fn parse_wait_ms(req: &Request) -> u64 {
    req.url()
        .ok()
        .and_then(|u| {
            u.query_pairs()
                .into_owned()
                .find(|(k, _)| k == "wait")
                .and_then(|(_, v)| v.parse::<u64>().ok())
        })
        .unwrap_or(0)
        .min(25)
        * 1000
}

// ---- CORS ----

fn is_allowed_origin(origin: &str) -> bool {
    origin == "https://renorma.app"
        || (origin.starts_with("https://") && origin.ends_with(".renorma.app"))
        || origin == "https://renorma-fit-dev.pages.dev"
        || origin == "https://renorma-admin-dev.pages.dev"
        || origin == "https://renorma-curator-dev.pages.dev"
        || origin.starts_with("http://localhost")
        || origin.starts_with("http://127.0.0.1")
}

fn add_cors(resp: Response, origin: &str) -> Result<Response> {
    let headers = Headers::new();
    if is_allowed_origin(origin) {
        let _ = headers.set("Access-Control-Allow-Origin", origin);
    }
    let _ = headers.set("Vary", "Origin");
    let _ = headers.set("Access-Control-Allow-Methods", "GET, POST, OPTIONS");
    let _ = headers.set("Access-Control-Allow-Headers", "Content-Type, Authorization");
    for (k, v) in resp.headers() {
        let _ = headers.set(&k, &v);
    }
    let status = resp.status_code();
    Ok(Response::from_body(resp.body().clone())?
        .with_headers(headers)
        .with_status(status))
}

/// Resolve EVERY required Store-bound secret at the top of the fetch entry. On the
/// FIRST failure, log the full reason loudly and return a 503 so every request to a
/// misconfigured worker is obviously broken (and says why) instead of degrading into
/// a confusing 401/500 deeper in the request.
async fn require_secrets(env: &Env) -> std::result::Result<(), Response> {
    for name in ["JWT_SECRET", "INTERNAL_PUSH_KEY", "ADMIN_APPROVE_SECRET"] {
        if let Err(reason) = token::secret_or_var(env, name).await {
            console_error!("STARTUP MISCONFIG: {name}: {reason}");
            let body = format!("MISCONFIGURED: {name} — {reason}");
            return Err(Response::error(body, 503)
                .unwrap_or_else(|_| Response::error("MISCONFIGURED", 503).unwrap()));
        }
    }
    Ok(())
}

/// Hourly cron (wrangler.toml [triggers]): the support digest push. Errors are
/// logged loudly — a failed run leaves the watermark untouched, so the next
/// hour's run re-covers the same messages.
#[event(scheduled)]
async fn scheduled(_event: ScheduledEvent, env: Env, _ctx: ScheduleContext) {
    match run_support_digest(&env).await {
        Ok(v) => console_log!("support digest: {v}"),
        Err(e) => console_error!("support digest FAILED: {e}"),
    }
}

#[event(fetch)]
async fn main(req: Request, env: Env, _ctx: Context) -> Result<Response> {
    let origin = req
        .headers()
        .get("Origin")
        .ok()
        .flatten()
        .unwrap_or_default();

    if req.method() == Method::Options {
        let headers = Headers::new();
        if is_allowed_origin(&origin) {
            let _ = headers.set("Access-Control-Allow-Origin", &origin);
        }
        let _ = headers.set("Vary", "Origin");
        let _ = headers.set("Access-Control-Allow-Methods", "GET, POST, OPTIONS");
        let _ = headers.set("Access-Control-Allow-Headers", "Content-Type, Authorization");
        let _ = headers.set("Access-Control-Max-Age", "86400");
        return Ok(Response::empty()?.with_headers(headers).with_status(204));
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
        return Ok(resp);
    }

    let router = Router::new();

    let result = router
        // USER side (JWT sub = user_id; user owns its DO via idFromName(sub))
        .post_async("/message", user_send)
        .get_async("/messages", user_messages)
        .post_async("/read", user_read)
        // EXPERT side (JWT AND sub DO-approved in the admins table)
        .get_async("/conversations", expert_conversations)
        .get_async("/conversations/:uid/messages", expert_messages)
        .post_async("/conversations/:uid/reply", expert_reply)
        .post_async("/conversations/:uid/read", expert_read)
        // CURATOR side (свободная регистрация; каждая операция — в своих клиентах)
        .post_async("/curator/register", curator_register)
        .get_async("/curator/me", curator_me)
        .post_async("/curator/me", curator_me_set)
        .get_async("/curator/clients", curator_clients)
        .post_async("/curator/clients", curator_client_create)
        .post_async("/curator/clients/:cid/rename", curator_client_rename)
        .post_async("/curator/clients/:cid/delete", curator_client_delete)
        .post_async("/curator/clients/:cid/unbind", curator_client_unbind)
        .get_async("/curator/clients/:cid/messages", curator_messages)
        .post_async("/curator/clients/:cid/reply", curator_reply)
        .post_async("/curator/clients/:cid/read", curator_read)
        .post_async("/curator/clients/:cid/request", curator_request_data)
        .get_async("/curator/clients/:cid/report", curator_report)
        // Сторона ХУДЕЮЩЕГО: приглашение, согласие, своя отвязка
        .get_async("/curator/invite/:code", curator_invite_peek)
        .post_async("/curator/invite/:code/accept", curator_invite_accept)
        .get_async("/curator/binding", curator_binding)
        .post_async("/curator/unbind", curator_unbind_by_user)
        // ADMIN authorization (request-code + operator secret; no redeploy to add)
        .get_async("/admin/me", admin_me)
        .post_async("/admin/request", admin_request)
        .post_async("/admin/approve", admin_approve)
        .post_async("/admin/digest-run", admin_digest_run)
        // INTERNAL: cross-worker admin check (payment-worker via service binding).
        .post_async("/internal/is-admin", internal_is_admin)
        .post_async("/internal/user-wipe", internal_user_wipe)
        .run(req, env)
        .await;

    match result {
        Ok(resp) => add_cors(resp, &origin),
        Err(e) => {
            let body = serde_json::json!({ "error": e.to_string() });
            let mut resp = Response::from_json(&body)?.with_status(500);
            let headers = resp.headers_mut();
            if is_allowed_origin(&origin) {
                let _ = headers.set("Access-Control-Allow-Origin", &origin);
            }
            let _ = headers.set("Vary", "Origin");
            Ok(resp)
        }
    }
}

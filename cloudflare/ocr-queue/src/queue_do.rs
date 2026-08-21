use serde::{Deserialize, Serialize};
use wasm_bindgen::JsValue;
use worker::*;

// Base64 chunk size kept well under the SQLite-backed DO per-value limit.
// MUST match the TS CHUNK exactly — a mismatch corrupts reassembled images.
const CHUNK: usize = 700_000;

/// A `processing` job whose `updated_at` hasn't advanced (no `/progress`
/// heartbeat, no `/complete`) for this long is treated as dropped by the poller
/// — the on-prem model hung, the poller restarted, or a `/complete` was lost —
/// and is requeued so it self-heals instead of showing «распознаётся» forever.
const STALE_MS: i64 = 120_000;
/// After this many requeues, fail the job instead of retrying forever (a poison
/// image that keeps hanging the model), so the client stops waiting.
const MAX_ATTEMPTS: i64 = 3;

/// Ключ с временем ПОСЛЕДНЕГО обращения поллера (мс эпохи). Пишется на каждом
/// `/claim` — поллер стучится туда независимо от того, есть ли работа.
const POLLER_SEEN_KEY: &str = "poller_seen_ms";

/// Молчание, после которого в Telegram уходит оповещение. Отдельно от окна
/// живости: маршрут переключается сразу, как только поллер выпал из минуты, а
/// будить человека стоит только когда стало ясно, что это не заминка.
const POLLER_ALERT_MS: i64 = 120_000;
/// Как часто сторож просыпается проверить молчание.
const WATCH_TICK_MS: i64 = 60_000;
/// Флаг «об этом простое уже оповестили» — чтобы сообщение было ОДНО, а не
/// каждую минуту. Снимается, когда поллер снова пришёл за работой.
const POLLER_ALERTED_KEY: &str = "poller_alerted";

/// Насколько давно поллер мог молчать, чтобы его всё ещё считать живым.
///
/// Поллер опрашивает `/claim` раз в POLL_INTERVAL (3 с) и ходит по очередям по
/// кругу, так что на ОДНУ очередь приходится удар раз в несколько секунд. Минуты
/// хватает, чтобы пережить перезапуск процесса и обрыв туннеля, и при этом не
/// гнать людей на платное распознавание, пока свой сервер жив.
const POLLER_ALIVE_MS: i64 = 60_000;

fn now_ms() -> i64 {
    Date::now().as_millis() as i64
}

/// Per-job record stored under `job:<id>`. Option fields are skipped when None so
/// the persisted JSON shape mirrors the TS `Job` interface exactly.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct Job {
    id: String,
    status: String, // "queued" | "processing" | "done" | "error"
    owner: String,
    /// The FULL prompt to run, built on the frontend (all business logic + the
    /// user's i18n language live there). The poller is a dumb executor: it runs
    /// this prompt against the on-prem model and returns the raw answer.
    /// `#[serde(default)]` so any job persisted before this field existed loads.
    #[serde(default)]
    prompt: String,
    chunks: usize,
    created_at: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    started_at: Option<i64>,
    updated_at: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    phase: Option<String>, // "thinking" | "answer"
    #[serde(default, skip_serializing_if = "Option::is_none")]
    thinking_tokens: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    answer_tokens: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    result: Option<serde_json::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    error: Option<String>,
    /// How many times this job has been (re)queued to the poller. Old records
    /// without the field deserialize as 0.
    #[serde(default)]
    attempts: i64,
}

#[durable_object]
pub struct QueueDO {
    state: worker::durable::State,
    env: Env,
}

impl QueueDO {
    async fn queue_ids(&self) -> Result<Vec<String>> {
        Ok(self
            .state
            .storage()
            .get::<Vec<String>>("queue")
            .await
            .ok()
            .flatten()
            .unwrap_or_default())
    }

    /// Ids currently handed to the poller (status `processing`). Mirrors `queue`;
    /// used to find dropped jobs to requeue (see [`requeue_stale`]).
    async fn processing_ids(&self) -> Result<Vec<String>> {
        Ok(self
            .state
            .storage()
            .get::<Vec<String>>("processing")
            .await
            .ok()
            .flatten()
            .unwrap_or_default())
    }

    /// Requeue (or, past [`MAX_ATTEMPTS`], fail) jobs stuck in `processing` with no
    /// update for [`STALE_MS`]. Called at the start of `/claim`, which the poller
    /// hits every few seconds, so a dropped job recovers within seconds without an
    /// alarm. A job still actively streaming `/progress` refreshes `updated_at` and
    /// is left alone.
    async fn requeue_stale(&self) -> Result<()> {
        let now = now_ms();
        let processing = self.processing_ids().await?;
        let mut still = Vec::new();
        let mut q = self.queue_ids().await?;
        let mut q_changed = false;
        for id in processing {
            let Some(mut job) = self.get_job(&id).await? else { continue };
            if job.status != "processing" {
                continue; // already terminal / requeued elsewhere → drop from list
            }
            if now - job.updated_at <= STALE_MS {
                still.push(id);
                continue;
            }
            job.attempts += 1;
            job.updated_at = now;
            if job.attempts > MAX_ATTEMPTS {
                job.status = "error".into();
                job.error = Some("recognition timed out".into());
                self.put_job(&id, &job).await?;
            } else {
                job.status = "queued".into();
                job.started_at = None;
                self.put_job(&id, &job).await?;
                q.push(id.clone());
                q_changed = true;
            }
        }
        self.state.storage().put("processing", &still).await?;
        if q_changed {
            self.state.storage().put("queue", &q).await?;
        }
        Ok(())
    }

    async fn get_job(&self, id: &str) -> Result<Option<Job>> {
        Ok(self
            .state
            .storage()
            .get::<Job>(&format!("job:{id}"))
            .await
            .ok()
            .flatten())
    }

    async fn put_job(&self, id: &str, job: &Job) -> Result<()> {
        self.state.storage().put(&format!("job:{id}"), job).await
    }

    /// Best-effort neuro-token usage report to payment-worker (the UsageDO owner)
    /// over the PAYMENT service binding. source="vision". NEVER propagates an
    /// error — billing is best-effort; on any failure we log loudly and swallow.
    async fn report_usage(&self, user_id: &str, tokens: i64) {
        if let Err(e) = self.try_report_usage(user_id, tokens).await {
            console_error!("usage report failed (vision, user={user_id}, tokens={tokens}): {e}");
        }
    }

    async fn try_report_usage(&self, user_id: &str, tokens: i64) -> Result<()> {
        let key = crate::token::secret_or_var(&self.env, "INTERNAL_PUSH_KEY")
            .await
            .map_err(Error::RustError)?;
        let headers = Headers::new();
        headers.set("X-Internal-Key", &key)?;
        headers.set("Content-Type", "application/json")?;
        // Vision runs on the on-prem GPU, NOT Cloudflare Workers AI — so it carries
        // NO Cloudflare neurons (inNeurons/outNeurons = 0). We record the model's
        // answer_tokens as output tokens for the volume/on-prem-load view only.
        let body = serde_json::json!({
            "userId": user_id,
            "source": "vision",
            "inTokens": 0,
            "outTokens": tokens,
            "inNeurons": 0,
            "outNeurons": 0,
        })
        .to_string();
        let mut init = RequestInit::new();
        init.with_method(Method::Post)
            .with_headers(headers)
            .with_body(Some(JsValue::from_str(&body)));
        // Host is irrelevant for a service-binding fetch; only the path routes.
        let req = Request::new_with_init("https://payment-worker/internal/usage", &init)?;
        let mut res = self.env.service("PAYMENT")?.fetch_request(req).await?;
        let status = res.status_code();
        if status != 200 {
            let text = res.text().await.unwrap_or_default();
            return Err(Error::RustError(format!("payment-worker returned {status}: {text}")));
        }
        Ok(())
    }

    /// Завести будильник сторожа, если он ещё не заведён.
    async fn arm_watch(&self) -> Result<()> {
        if self.state.storage().get_alarm().await?.is_none() {
            self.state
                .storage()
                .set_alarm(std::time::Duration::from_millis(WATCH_TICK_MS as u64))
                .await?;
        }
        Ok(())
    }

    /// Время последнего обращения поллера; 0 — не приходил ни разу.
    async fn poller_seen(&self) -> i64 {
        self.state
            .storage()
            .get::<i64>(POLLER_SEEN_KEY)
            .await
            .ok()
            .flatten()
            .unwrap_or(0)
    }

    /// Сказать в Telegram, что свой сервер молчит. Отправляет bug-report-worker —
    /// бот и чат настроены там, и знать о них этому воркеру незачем. Доставка
    /// best-effort: не дошло — строка в лог, очередь работает дальше.
    async fn alert_offline(&self, age_ms: i64) {
        let minutes = age_ms / 60_000;
        let text = format!(
            "Он-прем распознавание молчит: поллер не забирал работу {minutes} мин. \
             Картинки идут в платную модель."
        );
        if let Err(e) = self.post_alert(&text).await {
            console_error!("оповещение о простое поллера не ушло: {e}");
        }
    }

    async fn post_alert(&self, text: &str) -> Result<()> {
        let key = crate::token::secret_or_var(&self.env, "INTERNAL_PUSH_KEY")
            .await
            .map_err(Error::RustError)?;
        let headers = Headers::new();
        headers.set("Content-Type", "application/json")?;
        headers.set("X-Internal-Key", &key)?;
        let body = serde_json::json!({ "text": text }).to_string();
        let mut init = RequestInit::new();
        init.with_method(Method::Post)
            .with_headers(headers)
            .with_body(Some(JsValue::from_str(&body)));
        // Хост нужен именно этот: внутренние ручки bug-report-worker открыты только
        // для вызова через сервис-биндинг и проверяют его.
        let req = Request::new_with_init("https://bug-report-worker/internal/alert", &init)?;
        let mut res = self.env.service("BUG_REPORT")?.fetch_request(req).await?;
        let status = res.status_code();
        if status != 200 {
            let text = res.text().await.unwrap_or_default();
            return Err(Error::RustError(format!("bug-report-worker вернул {status}: {text}")));
        }
        Ok(())
    }
}

impl DurableObject for QueueDO {
    fn new(state: worker::durable::State, env: Env) -> Self {
        Self { state, env }
    }

    async fn fetch(&self, mut req: Request) -> Result<Response> {
        let url = req.url()?;
        let path = url.path().to_string();
        let method = req.method();

        // ---- /wipe-user (POST): drop every job (and any leftover image chunk)
        // belonging to one account. The DO is global, so we sweep by prefix and
        // match on the job's `owner`. ----
        if path == "/wipe-user" && method == Method::Post {
            let b: serde_json::Value = req.json().await?;
            let user_id = b
                .get("user_id")
                .and_then(|v| v.as_str())
                .ok_or_else(|| Error::RustError("missing user_id".into()))?
                .to_string();
            let storage = self.state.storage();
            let listed = storage
                .list_with_options(worker::durable::ListOptions::new().prefix("job:"))
                .await?;
            let mut victims: Vec<String> = Vec::new();
            for entry in listed.entries() {
                let entry = entry.map_err(|e| Error::RustError(format!("job entry: {e:?}")))?;
                let pair: Vec<serde_json::Value> = serde_wasm_bindgen::from_value(entry)
                    .map_err(|e| Error::RustError(format!("job pair: {e}")))?;
                let key = pair
                    .first()
                    .and_then(|k| k.as_str())
                    .ok_or_else(|| Error::RustError("job entry without key".into()))?;
                let val = pair
                    .get(1)
                    .ok_or_else(|| Error::RustError("job entry without value".into()))?;
                if val.get("owner").and_then(|o| o.as_str()) == Some(user_id.as_str()) {
                    victims.push(key.trim_start_matches("job:").to_string());
                }
            }
            let mut deleted = 0u32;
            for id in &victims {
                if storage.delete(&format!("job:{id}")).await? {
                    deleted += 1;
                }
                // Image chunks are normally dropped on /complete; a job killed
                // mid-flight can still hold some.
                let chunks = storage
                    .list_with_options(
                        worker::durable::ListOptions::new().prefix(&format!("img:{id}:")),
                    )
                    .await?;
                for entry in chunks.entries() {
                    let entry =
                        entry.map_err(|e| Error::RustError(format!("img entry: {e:?}")))?;
                    let pair: Vec<serde_json::Value> = serde_wasm_bindgen::from_value(entry)
                        .map_err(|e| Error::RustError(format!("img pair: {e}")))?;
                    if let Some(key) = pair.first().and_then(|k| k.as_str()) {
                        storage.delete(key).await?;
                    }
                }
            }
            return Response::from_json(&serde_json::json!({ "ok": true, "deleted": deleted }));
        }

        // ---- /enqueue (POST) ----
        if path == "/enqueue" && method == Method::Post {
            let b: serde_json::Value = req.json().await?;
            let id = b.get("id").and_then(|v| v.as_str()).unwrap_or("").to_string();
            let owner = b.get("owner").and_then(|v| v.as_str()).unwrap_or("").to_string();
            let prompt = b.get("prompt").and_then(|v| v.as_str()).unwrap_or("").to_string();
            let image_b64 = b
                .get("image_b64")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();

            // Chunk the base64 blob exactly as TS: slice by CHUNK *characters*.
            let chars: Vec<char> = image_b64.chars().collect();
            let mut n: usize = 0;
            let mut i = 0;
            while i < chars.len() {
                let end = (i + CHUNK).min(chars.len());
                let chunk: String = chars[i..end].iter().collect();
                self.state.storage().put(&format!("img:{id}:{n}"), chunk).await?;
                n += 1;
                i = end;
            }

            let job = Job {
                id: id.clone(),
                status: "queued".into(),
                owner,
                prompt,
                chunks: n,
                created_at: now_ms(),
                started_at: None,
                updated_at: now_ms(),
                phase: None,
                thinking_tokens: None,
                answer_tokens: None,
                result: None,
                error: None,
                attempts: 0,
            };
            self.put_job(&id, &job).await?;
            let mut q = self.queue_ids().await?;
            q.push(id);
            self.state.storage().put("queue", &q).await?;
            return Response::from_json(&serde_json::json!({ "ok": true }));
        }

        // ---- /poller-status ----
        // Жив ли он-прем: когда он в последний раз забирал работу. По этому ответу
        // приложение решает, слать картинку в очередь или платно на сторону.
        if path == "/poller-status" {
            let seen: i64 = self
                .state
                .storage()
                .get::<i64>(POLLER_SEEN_KEY)
                .await
                .ok()
                .flatten()
                .unwrap_or(0);
            let age = if seen > 0 { now_ms() - seen } else { -1 };
            let fired: i64 = self
                .state
                .storage()
                .get::<i64>("watch_fired_ms")
                .await
                .ok()
                .flatten()
                .unwrap_or(0);
            let alerted = self
                .state
                .storage()
                .get::<bool>(POLLER_ALERTED_KEY)
                .await
                .ok()
                .flatten()
                .unwrap_or(false);
            return Response::from_json(&serde_json::json!({
                "alive": seen > 0 && age >= 0 && age <= POLLER_ALIVE_MS,
                "last_seen_ms": seen,
                "age_ms": age,
                "alive_window_ms": POLLER_ALIVE_MS,
                "watch_fired_ms": fired,
                "alerted": alerted,
            }));
        }

        // ---- /claim ----
        if path == "/claim" {
            // Отметка живости: сам факт обращения поллера и есть его пульс.
            self.state.storage().put(POLLER_SEEN_KEY, now_ms()).await?;
            // Пришёл — значит следующий простой будет новым поводом сказать о нём.
            self.state.storage().put(POLLER_ALERTED_KEY, false).await?;
            // Сторож живёт цепочкой будильников и обрывается, когда оповещение уже
            // отправлено; возвращение поллера заводит цепочку заново.
            self.arm_watch().await?;
            // Self-heal first: recover jobs the poller dropped mid-flight.
            self.requeue_stale().await?;
            let mut q = self.queue_ids().await?;
            while !q.is_empty() {
                let id = q.remove(0);
                let job = self.get_job(&id).await?;
                let mut job = match job {
                    Some(j) if j.status == "queued" => j,
                    _ => continue,
                };
                job.status = "processing".into();
                job.started_at = Some(now_ms());
                job.updated_at = now_ms();
                self.put_job(&id, &job).await?;
                self.state.storage().put("queue", &q).await?;
                // Track as in-flight so a dropped job can be requeued later.
                let mut p = self.processing_ids().await?;
                if !p.contains(&id) {
                    p.push(id.clone());
                }
                self.state.storage().put("processing", &p).await?;
                return Response::from_json(&serde_json::json!({
                    "job_id": id,
                    "prompt": job.prompt,
                }));
            }
            self.state.storage().put("queue", &q).await?;
            return Response::from_json(&serde_json::json!({}));
        }

        // ---- /image ----
        if path == "/image" {
            let id = url
                .query_pairs()
                .find(|(k, _)| k == "id")
                .map(|(_, v)| v.to_string())
                .unwrap_or_default();
            let job = match self.get_job(&id).await? {
                Some(j) => j,
                None => return Response::error("not found", 404),
            };
            let mut b64 = String::new();
            for i in 0..job.chunks {
                let part: Option<String> =
                    self.state.storage().get::<String>(&format!("img:{id}:{i}")).await.ok().flatten();
                b64.push_str(&part.unwrap_or_default());
            }
            let headers = Headers::new();
            let _ = headers.set("Content-Type", "text/plain");
            return Ok(Response::ok(b64)?.with_headers(headers));
        }

        // ---- /progress (POST) ----
        if path == "/progress" && method == Method::Post {
            let b: serde_json::Value = req.json().await?;
            let job_id = b.get("job_id").and_then(|v| v.as_str()).unwrap_or("").to_string();
            let mut job = match self.get_job(&job_id).await? {
                Some(j) => j,
                None => {
                    return Ok(Response::from_json(&serde_json::json!({ "error": "unknown job" }))?
                        .with_status(404))
                }
            };
            job.phase = b.get("phase").and_then(|v| v.as_str()).map(String::from);
            if let Some(t) = b.get("thinking_tokens").and_then(|v| v.as_i64()) {
                job.thinking_tokens = Some(t);
            }
            if let Some(t) = b.get("answer_tokens").and_then(|v| v.as_i64()) {
                job.answer_tokens = Some(t);
            }
            job.updated_at = now_ms();
            self.put_job(&job_id, &job).await?;
            return Response::from_json(&serde_json::json!({ "ok": true }));
        }

        // ---- /complete (POST) ----
        if path == "/complete" && method == Method::Post {
            let b: serde_json::Value = req.json().await?;
            let job_id = b.get("job_id").and_then(|v| v.as_str()).unwrap_or("").to_string();
            let mut job = match self.get_job(&job_id).await? {
                Some(j) => j,
                None => {
                    return Ok(Response::from_json(&serde_json::json!({ "error": "unknown job" }))?
                        .with_status(404))
                }
            };
            let err = b.get("error");
            let err_truthy = err
                .map(|e| !e.is_null() && e.as_str() != Some("") && e.as_bool() != Some(false))
                .unwrap_or(false);
            if err_truthy {
                job.status = "error".into();
                // TS: `String(body.error)` — stringify whatever was sent.
                job.error = Some(match err.unwrap() {
                    serde_json::Value::String(s) => s.clone(),
                    other => other.to_string(),
                });
            } else {
                job.status = "done".into();
                job.result = b.get("result").cloned();
            }
            // The poller may deliver the final answer-token count on /complete
            // (not just via /progress) — take it if present.
            if let Some(t) = b.get("answer_tokens").and_then(|v| v.as_i64()) {
                job.answer_tokens = Some(t);
            }
            job.updated_at = now_ms();
            self.put_job(&job_id, &job).await?;
            // No longer in flight → drop from the processing list so the stale
            // sweep never touches a finished job.
            let mut p = self.processing_ids().await?;
            p.retain(|x| x != &job_id);
            self.state.storage().put("processing", &p).await?;
            // Free the image chunks once the job is finished.
            for i in 0..job.chunks {
                let _ = self.state.storage().delete(&format!("img:{job_id}:{i}")).await;
            }
            // Best-effort neuro-token usage report (source="vision"). NEVER fail
            // /complete on a reporting error — billing is best-effort.
            if job.status == "done" {
                if let Some(tokens) = job.answer_tokens {
                    if tokens > 0 && !job.owner.is_empty() {
                        self.report_usage(&job.owner, tokens).await;
                    }
                }
            }
            return Response::from_json(&serde_json::json!({ "ok": true }));
        }

        // ---- /status ----
        if path == "/status" {
            let id = url
                .query_pairs()
                .find(|(k, _)| k == "id")
                .map(|(_, v)| v.to_string())
                .unwrap_or_default();
            let job = match self.get_job(&id).await? {
                Some(j) => j,
                None => {
                    return Ok(Response::from_json(&serde_json::json!({ "error": "unknown job" }))?
                        .with_status(404))
                }
            };
            let mut position = 0i64;
            if job.status == "queued" {
                let q = self.queue_ids().await?;
                if let Some(idx) = q.iter().position(|x| x == &id) {
                    position = (idx as i64) + 1;
                }
            }
            return Response::from_json(&serde_json::json!({
                "status": job.status,
                "owner": job.owner,
                "position": position,
                "result": job.result.clone().unwrap_or(serde_json::Value::Null),
                "error": job.error.clone().map(serde_json::Value::String).unwrap_or(serde_json::Value::Null),
                "created_at": job.created_at,
                "started_at": job.started_at.map(|v| serde_json::json!(v)).unwrap_or(serde_json::Value::Null),
                "phase": job.phase.clone().map(serde_json::Value::String).unwrap_or(serde_json::Value::Null),
                "thinking_tokens": job.thinking_tokens.unwrap_or(0),
                "answer_tokens": job.answer_tokens.unwrap_or(0),
            }));
        }

        // ---- /tail (long-poll) ----
        if path == "/tail" {
            let id = url
                .query_pairs()
                .find(|(k, _)| k == "id")
                .map(|(_, v)| v.to_string())
                .unwrap_or_default();
            let since: i64 = url
                .query_pairs()
                .find(|(k, _)| k == "since")
                .map(|(_, v)| v.parse::<i64>().unwrap_or(0))
                .unwrap_or(0);
            let deadline = now_ms() + 20_000;
            loop {
                let job = match self.get_job(&id).await? {
                    Some(j) => j,
                    None => {
                        return Ok(Response::from_json(
                            &serde_json::json!({ "error": "unknown job" }),
                        )?
                        .with_status(404))
                    }
                };
                let terminal = job.status == "done" || job.status == "error";
                if job.updated_at > since || terminal || now_ms() >= deadline {
                    return Response::from_json(&serde_json::json!({
                        "status": job.status,
                        "phase": job.phase.clone().map(serde_json::Value::String).unwrap_or(serde_json::Value::Null),
                        "thinking_tokens": job.thinking_tokens.unwrap_or(0),
                        "answer_tokens": job.answer_tokens.unwrap_or(0),
                        "updated_at": job.updated_at,
                        "owner": job.owner,
                        "done": job.status == "done",
                        "error": job.error.clone().map(serde_json::Value::String).unwrap_or(serde_json::Value::Null),
                        "result": if job.status == "done" {
                            job.result.clone().unwrap_or(serde_json::Value::Null)
                        } else {
                            serde_json::Value::Null
                        },
                    }));
                }
                // Poll interval: 250ms (matches TS setTimeout).
                Delay::from(std::time::Duration::from_millis(250)).await;
            }
        }

        Response::error("Not found", 404)
    }

    /// СТОРОЖ ЗА СВОИМ СЕРВЕРОМ. Просыпается раз в минуту и смотрит, давно ли
    /// приходил поллер.
    ///
    /// Оповещение уходит ОДИН раз на простой: дальше будильник не заводится, и
    /// цепочку возобновляет сам поллер, когда вернётся за работой (`/claim` заодно
    /// снимает флаг). Пока он молчит, будить некого — сообщение уже отправлено.
    async fn alarm(&self) -> Result<Response> {
        let seen = self.poller_seen().await;
        // След срабатывания в самом хранилище: события будильника в `wrangler tail`
        // не показываются, и без этой отметки проверить сторожа нечем.
        self.state.storage().put("watch_fired_ms", now_ms()).await?;
        // Поллер не приходил ни разу: очередь только что развёрнута или её никто
        // не обслуживает. Тревожить нечем — не о чем сообщать.
        if seen == 0 {
            return Response::ok("");
        }
        let age = now_ms() - seen;
        if age < POLLER_ALERT_MS {
            self.state
                .storage()
                .set_alarm(std::time::Duration::from_millis(WATCH_TICK_MS as u64))
                .await?;
            return Response::ok("");
        }
        let alerted = self
            .state
            .storage()
            .get::<bool>(POLLER_ALERTED_KEY)
            .await
            .ok()
            .flatten()
            .unwrap_or(false);
        if !alerted {
            self.alert_offline(age).await;
            self.state.storage().put(POLLER_ALERTED_KEY, true).await?;
        }
        Response::ok("")
    }
}

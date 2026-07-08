use serde::{Deserialize, Serialize};
use wasm_bindgen::JsValue;
use worker::*;

// Base64 chunk size kept well under the SQLite-backed DO per-value limit.
// MUST match the TS CHUNK exactly — a mismatch corrupts reassembled images.
const CHUNK: usize = 700_000;

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
    custom_nutrients: Vec<serde_json::Value>,
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
        let body = serde_json::json!({
            "userId": user_id,
            "tokens": tokens,
            "source": "vision",
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
}

impl DurableObject for QueueDO {
    fn new(state: worker::durable::State, env: Env) -> Self {
        Self { state, env }
    }

    async fn fetch(&self, mut req: Request) -> Result<Response> {
        let url = req.url()?;
        let path = url.path().to_string();
        let method = req.method();

        // ---- /enqueue (POST) ----
        if path == "/enqueue" && method == Method::Post {
            let b: serde_json::Value = req.json().await?;
            let id = b.get("id").and_then(|v| v.as_str()).unwrap_or("").to_string();
            let owner = b.get("owner").and_then(|v| v.as_str()).unwrap_or("").to_string();
            let custom_nutrients = b
                .get("custom_nutrients")
                .and_then(|v| v.as_array())
                .cloned()
                .unwrap_or_default();
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
                custom_nutrients,
                chunks: n,
                created_at: now_ms(),
                started_at: None,
                updated_at: now_ms(),
                phase: None,
                thinking_tokens: None,
                answer_tokens: None,
                result: None,
                error: None,
            };
            self.put_job(&id, &job).await?;
            let mut q = self.queue_ids().await?;
            q.push(id);
            self.state.storage().put("queue", &q).await?;
            return Response::from_json(&serde_json::json!({ "ok": true }));
        }

        // ---- /claim ----
        if path == "/claim" {
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
                return Response::from_json(&serde_json::json!({
                    "job_id": id,
                    "custom_nutrients": job.custom_nutrients,
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
}

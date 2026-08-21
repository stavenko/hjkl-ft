// ai-worker — a thin, model-parametrized proxy over Workers AI.
//
// This is the single AI entrypoint. Callers pass the `model` and the messages
// (multimodal `image_url` parts included), so both nutrition text lookups and
// label-vision requests go through here. The worker does NO response parsing:
// the raw Workers AI output (stream or JSON) is passed straight through; the
// FRONTEND assembles and parses the fully-received content. (Previously a TS
// version re-parsed each SSE chunk and re-emitted it from a corrupted field,
// which silently mangled numbers/quotes mid-stream — do NOT regress that.)
//
// It owns NO Durable Object: it only BINDS the cross-script SUBSCRIPTION_DO that
// payment-worker owns, to gate AI on an active Trial/Paid subscription.

use wasm_bindgen::JsCast;
use worker::*;

mod token;
mod types;

use token::{secret_or_var, validate_from_header};

/// Invoke the Workers AI binding with the input as a REAL JS object graph.
///
/// `worker`'s `Ai::run`/`run_bytes` serialize the input via `serde_wasm_bindgen`,
/// which turns a serde MAP — our `serde_json::Value::Object` — into a JS `Map`.
/// `env.AI.run` reads `input.messages` as an OBJECT property, which a `Map` does
/// not expose, so Workers AI sees no `messages`/`prompt`/`requests` at the root and
/// rejects EVERY request with `5006: oneOf at '/' not met`. `JSON.parse` yields
/// plain objects all the way down, which the binding accepts. Returns the raw
/// resolved value: a result object for `stream:false`, a `ReadableStream` for
/// `stream:true`.
async fn ai_run(env: &Env, model: &str, params: &serde_json::Value) -> Result<wasm_bindgen::JsValue> {
    use wasm_bindgen::JsValue;
    let ai = env.ai("AI")?;
    let binding: &JsValue = ai.as_ref();
    let run_fn: js_sys::Function = js_sys::Reflect::get(binding, &JsValue::from_str("run"))
        .map_err(|e| Error::RustError(format!("AI.run lookup: {e:?}")))?
        .dyn_into()
        .map_err(|_| Error::RustError("AI.run is not a function".into()))?;
    let input = js_sys::JSON::parse(&serde_json::to_string(params)?)
        .map_err(|e| Error::RustError(format!("input JSON.parse: {e:?}")))?;
    let promise = run_fn
        .call2(binding, &JsValue::from_str(model), &input)
        .map_err(|e| Error::RustError(format!("AI.run call: {e:?}")))?;
    worker::wasm_bindgen_futures::JsFuture::from(js_sys::Promise::from(promise))
        .await
        .map_err(|e| Error::RustError(format!("AI.run: {e:?}")))
}

// ── CORS ────────────────────────────────────────────────────────────────────
// Known origins only (no wildcard): the prod app + any renorma.app subdomain,
// the dev test env, and localhost/127.0.0.1 for development. Mirrors the TS
// ALLOWED_ORIGIN_RE regex.
fn is_allowed_origin(origin: &str) -> bool {
    origin == "https://renorma.app"
        || (origin.starts_with("https://") && origin.ends_with(".renorma.app"))
        || origin == "https://renorma-fit-dev.pages.dev"
        || origin.starts_with("http://localhost")
        || origin.starts_with("http://127.0.0.1")
}

fn add_cors(resp: Response, origin: &str) -> Result<Response> {
    let headers = Headers::new();
    if is_allowed_origin(origin) {
        let _ = headers.set("Access-Control-Allow-Origin", origin);
    }
    let _ = headers.set("Vary", "Origin");
    for (k, v) in resp.headers() {
        let _ = headers.set(&k, &v);
    }
    let status = resp.status_code();
    Ok(Response::from_body(resp.body().clone())?
        .with_headers(headers)
        .with_status(status))
}

const CORS_METHODS: &str = "GET, POST, OPTIONS";
const CORS_HEADERS: &str = "Content-Type, Authorization";

// ── error helpers ─────────────────────────────────────────────────────────────
fn error_response(message: &str, status: u16) -> Response {
    Response::from_json(&serde_json::json!({ "error": message }))
        .expect("serialize error")
        .with_status(status)
}

/// Resolve every REQUIRED Store-bound secret at the top of the fetch entry. On the
/// first failure: log the full reason loudly and return a 503 so ANY request makes
/// the misconfiguration obvious (Workers have no separate startup — per-request is
/// intended).
async fn require_secrets(env: &Env) -> std::result::Result<(), Response> {
    for name in ["JWT_SECRET"] {
        if let Err(reason) = secret_or_var(env, name).await {
            console_error!("STARTUP MISCONFIG: {name}: {reason}");
            let body = format!("MISCONFIGURED: {name} — {reason}");
            return Err(
                Response::error(body, 503).unwrap_or_else(|_| Response::error("MISCONFIGURED", 503).unwrap()),
            );
        }
    }
    Ok(())
}

/// True if the user's subscription is active (Trial not expired, or Paid).
///
/// Delegates to payment-worker — the OWNER of the SubscriptionDO — over the
/// PAYMENT service binding, forwarding the caller's `Authorization` header (the
/// same JWT payment-worker validates). payment-worker resolves the DO by its own
/// private epoch, so this worker holds NO knowledge of the DO's name/epoch. That
/// coupling drifted once (this reader stuck on an old epoch → empty DO →
/// active:false → spurious 402 paywall); delegating removes it entirely.
async fn subscription_active(env: &Env, authorization: &str) -> Result<bool> {
    let headers = Headers::new();
    headers.set("Authorization", authorization)?;
    let mut init = RequestInit::new();
    init.with_method(Method::Get).with_headers(headers);
    // Host is irrelevant for a service-binding fetch; only the path routes.
    let req = Request::new_with_init("https://payment-worker/subscription", &init)?;
    let mut res = env.service("PAYMENT")?.fetch_request(req).await?;
    if res.status_code() < 200 || res.status_code() >= 300 {
        return Ok(false);
    }
    let status: serde_json::Value = res.json().await?;
    Ok(status.get("active").and_then(|v| v.as_bool()) == Some(true))
}

#[event(fetch)]
async fn main(req: Request, env: Env, ctx: Context) -> Result<Response> {
    let origin = req
        .headers()
        .get("Origin")
        .ok()
        .flatten()
        .unwrap_or_default();

    if req.method() == Method::Options {
        let headers = Headers::new();
        let _ = headers.set("Access-Control-Allow-Methods", CORS_METHODS);
        let _ = headers.set("Access-Control-Allow-Headers", CORS_HEADERS);
        // OPTIONS short-circuit happens before CORS-origin echo in the TS inner
        // handler, but the outer applyCors wrapper still echoes the origin + Vary.
        return add_cors(
            Response::empty()?.with_headers(headers).with_status(204),
            &origin,
        );
    }

    // Unauthenticated liveness probe: the frontend's connectivity check (see the
    // `net` service) hits this to decide "is the server reachable" — the AI worker
    // is the critical one, so its reachability drives the app's online flag. Kept
    // before JWT/secret checks so it stays a cheap, always-answerable 200. CORS is
    // WILDCARD (not the restricted echo) because it's a public liveness check with
    // no secrets, and it must answer probes from ANY origin — including the
    // per-deploy Pages hash subdomains that aren't in the allow-list.
    if req.method() == Method::Get && req.url().map(|u| u.path() == "/health").unwrap_or(false) {
        let headers = Headers::new();
        let _ = headers.set("Access-Control-Allow-Origin", "*");
        let _ = headers.set("Cache-Control", "no-store");
        return Ok(Response::ok("ok")?.with_headers(headers));
    }

    if let Err(resp) = require_secrets(&env).await {
        return add_cors(resp, &origin);
    }

    let resp = match handle(req, &env, &ctx).await {
        Ok(r) => r,
        Err(e) => error_response(&e.to_string(), 500),
    };
    add_cors(resp, &origin)
}

async fn handle(req: Request, env: &Env, ctx: &Context) -> Result<Response> {
    // JWT verify (Authorization: Bearer). 401 on missing/invalid. Keep the sub
    // (authenticated user_id) for backend-authoritative token accounting.
    let user_id = match validate_from_header(&req, env).await {
        Ok(sub) => sub,
        Err(_) => return Ok(error_response("Unauthorized", 401)),
    };

    let url = req.url()?;
    let path = url.path().to_string();

    // The TS verifies JWT, THEN rejects any non-POST with 404 (before path match).
    if req.method() != Method::Post {
        return Ok(error_response("Not found", 404));
    }

    if path == "/chat/completions" {
        // Gate AI on an active subscription (Trial not expired, or Paid). The
        // bearer is forwarded to payment-worker, which owns the subscription.
        let authorization = req.headers().get("Authorization")?.unwrap_or_default();
        if !subscription_active(env, &authorization).await? {
            return Ok(error_response("subscription_required", 402));
        }
        return handle_chat_completions(req, env, ctx, &user_id).await;
    }

    Ok(error_response("Not found", 404))
}

// ── Neuro-token usage accounting (best-effort) ────────────────────────────────
// Report backend-authoritative TEXT token consumption to payment-worker, which
// owns the global UsageDO billing store. This path is BEST-EFFORT: it must NEVER
// fail the user's AI request. On any error we log loudly (console_error!) and
// swallow. INTERNAL_PUSH_KEY-guarded; if that key is unset we skip + log.

/// Micro-neurons (neurons × 1e6) per token, as (input, output), for each model.
/// Cloudflare prices Workers AI in NEURONS per MILLION tokens — which is exactly
/// micro-neurons PER TOKEN, so the cost is an exact integer: prompt·in + completion·out.
/// Input and output are billed at different rates. Keep in sync with
/// https://developers.cloudflare.com/workers-ai/platform/pricing/
fn neuron_rates(model: &str) -> (i64, i64) {
    match model {
        "@cf/qwen/qwen3-30b-a3b-fp8" => (4_625, 30_475),
        // Only qwen3-30b is used for text today; price unknown models at its rates
        // as a best-effort estimate (the caller logs the unknown model).
        _ => (4_625, 30_475),
    }
}

/// (prompt_tokens, completion_tokens) from a value carrying a top-level `usage`.
/// Falls back to attributing a lone total to OUTPUT (the pricier rate → conservative
/// upper bound). None when no positive count.
fn usage_split(val: &serde_json::Value) -> Option<(i64, i64)> {
    let usage = val.get("usage")?;
    let p = usage.get("prompt_tokens").and_then(|v| v.as_i64());
    let c = usage.get("completion_tokens").and_then(|v| v.as_i64());
    match (p, c) {
        (Some(p), Some(c)) if p + c > 0 => Some((p.max(0), c.max(0))),
        _ => match usage.get("total_tokens").and_then(|v| v.as_i64()) {
            Some(t) if t > 0 => Some((0, t)),
            _ => None,
        },
    }
}

/// POST usage to payment-worker over the PAYMENT service binding. Records total
/// tokens AND the Cloudflare-billable NEURONS (input·in_rate + output·out_rate,
/// micro-neurons). Best-effort: logs and swallows every failure; skips (logs) if
/// INTERNAL_PUSH_KEY is unset.
async fn report_usage(env: &Env, user_id: &str, model: &str, prompt: i64, completion: i64) {
    let (prompt, completion) = (prompt.max(0), completion.max(0));
    if prompt + completion <= 0 || user_id.is_empty() {
        return;
    }
    let (in_rate, out_rate) = neuron_rates(model);
    // micro-neurons (neurons × 1e6): rate is "neurons per M tokens" == µ-neurons/token.
    let in_neurons = prompt * in_rate;
    let out_neurons = completion * out_rate;
    let key = match secret_or_var(env, "INTERNAL_PUSH_KEY").await {
        Ok(k) => k,
        Err(reason) => {
            console_error!("usage-report skipped: INTERNAL_PUSH_KEY unset: {reason}");
            return;
        }
    };
    if let Err(e) =
        report_usage_inner(env, &key, user_id, "text", prompt, completion, in_neurons, out_neurons).await
    {
        console_error!("usage-report failed (best-effort, swallowed): {e:?}");
    }
}

#[allow(clippy::too_many_arguments)]
async fn report_usage_inner(
    env: &Env,
    key: &str,
    user_id: &str,
    source: &str,
    in_tokens: i64,
    out_tokens: i64,
    in_neurons: i64,
    out_neurons: i64,
) -> Result<()> {
    let headers = Headers::new();
    headers.set("X-Internal-Key", key)?;
    headers.set("Content-Type", "application/json")?;
    let body = serde_json::json!({
        "userId": user_id, "source": source,
        "inTokens": in_tokens, "outTokens": out_tokens,
        "inNeurons": in_neurons, "outNeurons": out_neurons,
    });
    let mut init = RequestInit::new();
    init.with_method(Method::Post)
        .with_headers(headers)
        .with_body(Some(wasm_bindgen::JsValue::from_str(&body.to_string())));
    // Host is irrelevant for a service-binding fetch; only the path routes.
    let req = Request::new_with_init("https://payment-worker/internal/usage", &init)?;
    let res = env.service("PAYMENT")?.fetch_request(req).await?;
    let status = res.status_code();
    if status < 200 || status >= 300 {
        return Err(Error::RustError(format!("/internal/usage → {status}")));
    }
    Ok(())
}

/// Scan ONE complete SSE line for the chunk carrying a top-level `usage`.
/// `Some((prompt, completion))` when this line is that chunk.
fn usage_from_sse_line(line: &str) -> Option<(i64, i64)> {
    let data = line.strip_prefix("data:")?.trim();
    if data.is_empty() || data == "[DONE]" {
        return None;
    }
    usage_split(&serde_json::from_str::<serde_json::Value>(data).ok()?)
}

/// The answer text carried by ONE complete SSE line, if it carries any.
fn content_from_sse_line(line: &str) -> Option<String> {
    let data = line.strip_prefix("data:")?.trim();
    if data.is_empty() || data == "[DONE]" {
        return None;
    }
    let v: serde_json::Value = serde_json::from_str(data).ok()?;
    let c = v.get("choices")?.get(0)?.get("delta")?.get("content")?.as_str()?;
    (!c.is_empty()).then(|| c.to_string())
}

/// Сколько подряд идущих `!` считать срывом генерации.
const RUNAWAY_BANGS: usize = 10;

/// Аварийный хвост потока: клиенту говорится, что ответ не состоялся, и поток
/// закрывается. Ответ обрывается НАМЕРЕННО — дальше модель всё равно выдаёт мусор,
/// а платим мы за каждый выданный токен.
fn runaway_sse_tail() -> Vec<u8> {
    let payload = serde_json::json!({
        "error": {
            "type": "runaway_generation",
            "message": format!(
                "поток оборван: модель выдала {RUNAWAY_BANGS} восклицательных знаков подряд"
            ),
        },
        "choices": [{ "index": 0, "delta": {}, "finish_reason": "runaway" }],
    });
    format!("data: {payload}\n\ndata: [DONE]\n\n").into_bytes()
}


// ── Сторонний провайдер (thirdparty) ──────────────────────────────────────────
//
// Картинки на Workers AI не идут: там нет модели, которая читает этикетку так, как
// нам нужно. Поэтому запрос с чужой моделью уходит по HTTP наружу — по тому же
// OpenAI-совместимому протоколу, адрес и ключ берутся из переменных воркера.
// Ключ НИКОГДА не покидает воркер: клиент шлёт только имя модели, а подписку он к
// этому моменту уже прошёл (гейт стоит выше по стеку).

/// Модель Cloudflare Workers AI. Всё остальное — сторонний провайдер.
fn is_workers_ai(model: &str) -> bool {
    model.starts_with("@cf/")
}

/// РЕЕСТР сторонних провайдеров: какая модель куда идёт и ИМЕНЕМ какого секрета
/// открывается. Сам реестр — тоже секрет (в проде из глобального Secrets Store),
/// поэтому ни адреса, ни имена ключей в репозитории не лежат. Формат — массив:
///
/// ```json
/// [
///   {"models":["gpt-4o-mini"],"url":"https://api.openai.com/v1/chat/completions","key":"OPENAI_API_KEY"},
///   {"models":["*"],"url":"https://openrouter.ai/api/v1/chat/completions","key":"OPENROUTER_API_KEY"}
/// ]
/// ```
///
/// `key` — ИМЯ биндинга, под которым сам ключ лежит в Secrets Store (в деве —
/// var/secret воркера с тем же именем). Ключи в реестре не хранятся: там только
/// их именование.
const THIRDPARTY_PROVIDERS: &str = "THIRDPARTY_PROVIDERS";

/// Один провайдер из реестра.
#[derive(serde::Deserialize)]
struct Provider {
    /// Модели этого провайдера. `*` — «любая»: такой провайдер берётся, только
    /// если модель не назвал никто поимённо.
    #[serde(default)]
    models: Vec<String>,
    /// ПОЛНЫЙ адрес ручки, вместе с `/chat/completions`.
    url: String,
    /// Имя секрета с ключом провайдера.
    key: String,
}

/// Провайдер для модели: сначала поимённое совпадение, затем `*`.
fn pick_provider<'a>(providers: &'a [Provider], model: &str) -> Option<&'a Provider> {
    providers
        .iter()
        .find(|p| p.models.iter().any(|m| m == model))
        .or_else(|| providers.iter().find(|p| p.models.iter().any(|m| m == "*")))
}

/// Поля запроса, которые имеют смысл у стороннего провайдера. Всё
/// Workers-AI-специфичное (`think`, `chat_template_kwargs`) отбрасывается.
const THIRDPARTY_FIELDS: [&str; 5] = ["model", "messages", "max_tokens", "temperature", "top_p"];

/// Привести наш `response_format` к виду OpenAI: там у `json_schema` ОБЯЗАТЕЛЬНО
/// есть `name`, а схема должна быть без `$ref` (провайдеры их не разворачивают).
///
/// `strict` просим явно: у Workers AI схема ЗАДАЁТ грамматику, а у стороннего
/// провайдера без этого флага она читается как пожелание.
fn openai_response_format(rf: &serde_json::Value) -> Option<serde_json::Value> {
    let schema = rf.get("json_schema").and_then(|js| js.get("schema"))?;
    Some(serde_json::json!({
        "type": "json_schema",
        "json_schema": { "name": "answer", "schema": inline_schema(schema), "strict": true },
    }))
}

/// Та же инструкция о форме, что уходит в промпт на пути Workers AI, плюс запрет
/// заворачивать ответ в массив.
///
/// Провайдер соблюдает схему не как грамматику: замер на qwen3.8-27b поймал, что
/// примерно каждый третий ответ приезжает как `[{…}]` вместо `{…}` — поля те же,
/// форма другая, и разбор на стороне приложения падает.
fn thirdparty_json_instruction(schema: &serde_json::Value) -> String {
    let inlined = strip_schema_meta(&inline_schema(schema));
    let schema_json = serde_json::to_string(&inlined).unwrap_or_default();
    format!(
        "\n\nYou MUST respond with ONLY valid JSON (no markdown, no explanation, no code fences). \
Respond with ONE object, never an array of objects. \
The JSON MUST conform to this exact schema:\n{schema_json}"
    )
}

/// Дописать инструкцию к первому системному сообщению со строковым содержимым, а
/// если такого нет — поставить системное сообщение в начало.
fn append_system_instruction(messages: &mut Vec<serde_json::Value>, text: &str) {
    let sys = messages
        .iter()
        .position(|m| m.get("role").and_then(|r| r.as_str()) == Some("system"))
        .filter(|i| messages[*i].get("content").and_then(|c| c.as_str()).is_some());
    match sys {
        Some(i) => {
            let content = messages[i]
                .get("content")
                .and_then(|c| c.as_str())
                .unwrap_or_default()
                .to_string();
            let mut m = messages[i].as_object().cloned().unwrap_or_default();
            m.insert(
                "content".to_string(),
                serde_json::Value::String(format!("{content}{text}")),
            );
            messages[i] = serde_json::Value::Object(m);
        }
        None => messages.insert(
            0,
            serde_json::json!({ "role": "system", "content": format!("You are a helpful assistant.{text}") }),
        ),
    }
}

/// Расход у стороннего провайдера считается В ТОКЕНАХ: нейронов там нет, это не
/// Cloudflare. Пишем их отдельным источником (`vision`), чтобы не подмешивать в
/// нейроны Workers AI. Best-effort, как и текстовый учёт.
async fn report_thirdparty_usage(env: &Env, user_id: &str, prompt: i64, completion: i64) {
    let (prompt, completion) = (prompt.max(0), completion.max(0));
    if prompt + completion <= 0 || user_id.is_empty() {
        return;
    }
    let key = match secret_or_var(env, "INTERNAL_PUSH_KEY").await {
        Ok(k) => k,
        Err(reason) => {
            console_error!("usage-report skipped: INTERNAL_PUSH_KEY unset: {reason}");
            return;
        }
    };
    if let Err(e) =
        report_usage_inner(env, &key, user_id, "vision", prompt, completion, 0, 0).await
    {
        console_error!("usage-report failed (best-effort, swallowed): {e:?}");
    }
}

/// Проксировать запрос стороннему провайдеру. Стрим отдаётся клиенту байт в байт
/// (как и путь Workers AI — ничего не переразбираем), по дороге строки SSE
/// просматриваются ради последнего чанка с `usage`.
async fn proxy_thirdparty(
    body: serde_json::Value,
    env: &Env,
    ctx: &Context,
    user_id: &str,
    model: &str,
) -> Result<Response> {
    let registry = match secret_or_var(env, THIRDPARTY_PROVIDERS).await {
        Ok(r) => r,
        Err(reason) => {
            console_error!("thirdparty: {reason}");
            return Ok(error_response("thirdparty_not_configured", 503));
        }
    };
    let providers: Vec<Provider> = match serde_json::from_str(&registry) {
        Ok(p) => p,
        Err(e) => {
            console_error!("thirdparty: реестр {THIRDPARTY_PROVIDERS} не разбирается: {e}");
            return Ok(error_response("thirdparty_not_configured", 503));
        }
    };
    let Some(provider) = pick_provider(&providers, model) else {
        console_error!("thirdparty: модель '{model}' не значится ни у одного провайдера");
        return Ok(error_response(&format!("unknown model: {model}"), 400));
    };
    let url = provider.url.clone();
    // Ключ лежит под ИМЕНЕМ из реестра: в проде это биндинг Secrets Store, в деве —
    // var/secret воркера.
    let key = match secret_or_var(env, &provider.key).await {
        Ok(k) => k,
        Err(reason) => {
            console_error!("thirdparty: ключ '{}': {reason}", provider.key);
            return Ok(error_response("thirdparty_not_configured", 503));
        }
    };

    let mut out = serde_json::Map::new();
    for field in THIRDPARTY_FIELDS {
        if let Some(v) = body.get(field) {
            out.insert(field.to_string(), v.clone());
        }
    }
    // Схема уходит провайдеру ДВАЖДЫ: полем `response_format` и текстом в промпте —
    // ровно как на пути Workers AI. Одного поля мало: у стороннего оно соблюдается
    // не всегда, и ответ приезжает то объектом, то массивом объектов.
    if let Some(rf) = body.get("response_format") {
        if let Some(schema) = rf.get("json_schema").and_then(|js| js.get("schema")) {
            let mut messages: Vec<serde_json::Value> = body
                .get("messages")
                .and_then(|m| m.as_array())
                .cloned()
                .unwrap_or_default();
            append_system_instruction(&mut messages, &thirdparty_json_instruction(schema));
            out.insert("messages".to_string(), serde_json::Value::Array(messages));
        }
        if let Some(rf) = openai_response_format(rf) {
            out.insert("response_format".to_string(), rf);
        }
    }
    // РАССУЖДЕНИЕ. У Workers AI им управляет `chat_template_kwargs`, у Alibaba —
    // top-level `enable_thinking`. Наши вызовы говорят про это одним флагом `think`,
    // так что переводим его, а явный `enable_thinking` от клиента уважаем как есть.
    // Гибридным qwen3 (32b, 30b-a3b, 235b-a22b) это не роскошь: без `false` они
    // отказывают на не-потоковом запросе с 400.
    if let Some(v) = body.get("enable_thinking").or_else(|| body.get("think")) {
        if let Some(b) = v.as_bool() {
            out.insert("enable_thinking".to_string(), serde_json::Value::Bool(b));
        }
    }

    let want_stream = body.get("stream").and_then(|s| s.as_bool()).unwrap_or(true);
    out.insert("stream".to_string(), serde_json::Value::Bool(want_stream));
    if want_stream {
        out.insert(
            "stream_options".to_string(),
            serde_json::json!({ "include_usage": true }),
        );
    }

    let headers = Headers::new();
    headers.set("Authorization", &format!("Bearer {key}"))?;
    headers.set("Content-Type", "application/json")?;
    let mut init = RequestInit::new();
    init.with_method(Method::Post)
        .with_headers(headers)
        .with_body(Some(wasm_bindgen::JsValue::from_str(
            &serde_json::Value::Object(out).to_string(),
        )));
    let req = Request::new_with_init(&url, &init)?;
    let mut res = Fetch::Request(req).send().await?;

    let status = res.status_code();
    if status < 200 || status >= 300 {
        // Причину показываем как есть: 401/429/400 провайдера должны быть видны в
        // логе и в приложении, а не прятаться за общей пятисоткой.
        let text = res.text().await.unwrap_or_default();
        console_error!("thirdparty → {status}: {text}");
        return Ok(error_response(&format!("thirdparty {status}: {text}"), status));
    }

    if !want_stream {
        let val: serde_json::Value = res.json().await?;
        if let Some((p, c)) = usage_split(&val) {
            let env = env.clone();
            let user_id = user_id.to_string();
            ctx.wait_until(async move { report_thirdparty_usage(&env, &user_id, p, c).await });
        }
        return Response::from_json(&val);
    }

    // Один потребитель, как и на пути Workers AI: байты идут клиенту нетронутыми,
    // а по дороге собираются полные строки SSE — из них берётся последний `usage`.
    let (tx, mut rx) = futures_channel::mpsc::unbounded::<(i64, i64)>();
    {
        let env = env.clone();
        let user_id = user_id.to_string();
        ctx.wait_until(async move {
            let mut last: Option<(i64, i64)> = None;
            while let Some(u) = futures_util::StreamExt::next(&mut rx).await {
                last = Some(u);
            }
            match last {
                Some((p, c)) => report_thirdparty_usage(&env, &user_id, p, c).await,
                None => console_error!("usage-report: no usage chunk in thirdparty stream"),
            }
        });
    }

    let watched = futures_util::StreamExt::scan(
        res.stream()?,
        Vec::<u8>::new(),
        move |carry, chunk: Result<Vec<u8>>| {
            if let Ok(bytes) = &chunk {
                carry.extend_from_slice(bytes);
                let mut consumed = 0usize;
                for line in carry.split_inclusive(|b| *b == b'\n') {
                    if !line.ends_with(b"\n") {
                        break;
                    }
                    consumed += line.len();
                    if let Some(u) = usage_from_sse_line(&String::from_utf8_lossy(line)) {
                        let _ = tx.unbounded_send(u);
                    }
                }
                carry.drain(..consumed.min(carry.len()));
                if carry.len() > 64 * 1024 {
                    carry.clear();
                }
            }
            futures_util::future::ready(Some(chunk))
        },
    );
    let resp = Response::from_stream(watched)?;
    let headers = resp.headers();
    headers.set("Content-Type", "text/event-stream")?;
    headers.set("Cache-Control", "no-cache")?;
    Ok(resp)
}

// ── Chat completions request massaging ────────────────────────────────────────

/// Recursively resolve `$ref` (`#/$defs/X` or `#/definitions/X`) against `defs`
/// and strip the meta keys `$defs`/`definitions`/`$schema`/`title`. Ported 1:1
/// from the TS `resolveRefs`.
fn resolve_refs(node: &serde_json::Value, defs: &serde_json::Map<String, serde_json::Value>) -> serde_json::Value {
    match node {
        serde_json::Value::Array(arr) => {
            serde_json::Value::Array(arr.iter().map(|item| resolve_refs(item, defs)).collect())
        }
        serde_json::Value::Object(obj) => {
            if let Some(serde_json::Value::String(ref_path)) = obj.get("$ref") {
                let def_name = ref_path
                    .replace("#/$defs/", "")
                    .replace("#/definitions/", "");
                if let Some(resolved) = defs.get(&def_name) {
                    return resolve_refs(resolved, defs);
                }
                // Unresolved: return the object as-is (matches TS `return obj`).
                return serde_json::Value::Object(obj.clone());
            }
            let mut result = serde_json::Map::new();
            for (key, value) in obj.iter() {
                if key == "$defs" || key == "definitions" || key == "$schema" || key == "title" {
                    continue;
                }
                result.insert(key.clone(), resolve_refs(value, defs));
            }
            serde_json::Value::Object(result)
        }
        other => other.clone(),
    }
}

fn inline_schema(schema: &serde_json::Value) -> serde_json::Value {
    let empty = serde_json::Map::new();
    let defs = schema
        .get("$defs")
        .or_else(|| schema.get("definitions"))
        .and_then(|v| v.as_object())
        .unwrap_or(&empty);
    resolve_refs(schema, defs)
}

/// Убрать из схемы служебное, прежде чем показывать её МОДЕЛИ: `$schema`, `title`,
/// `$defs`/`definitions`.
///
/// Схема вклеивается в промпт текстом и читается наравне с инструкцией, поэтому всё
/// лишнее в ней — шум. Замер на выдуманных именах: со служебными полями шесть
/// бессмысленных слов из пятнадцати проходили как еда («Бубурек копчёный — a type of
/// smoked bread», уверенность 0.90), без них — ни одного. Название типа из Rust
/// («IdentityAnswer») модели не говорит ничего, а место в голове занимает.
///
/// В саму Workers AI уходит ПОЛНАЯ схема: там она задаёт грамматику, и служебные
/// поля ей не мешают.
fn strip_schema_meta(v: &serde_json::Value) -> serde_json::Value {
    match v {
        serde_json::Value::Object(map) => {
            let cleaned = map
                .iter()
                .filter(|(k, _)| !matches!(k.as_str(), "$schema" | "title" | "$defs" | "definitions"))
                .map(|(k, val)| (k.clone(), strip_schema_meta(val)))
                .collect();
            serde_json::Value::Object(cleaned)
        }
        serde_json::Value::Array(items) => {
            serde_json::Value::Array(items.iter().map(strip_schema_meta).collect())
        }
        other => other.clone(),
    }
}

/// True if any message has array content with an `image_url` part.
fn has_image_content(messages: &[serde_json::Value]) -> bool {
    messages.iter().any(|m| {
        m.get("content")
            .and_then(|c| c.as_array())
            .map(|parts| {
                parts
                    .iter()
                    .any(|p| p.get("type").and_then(|t| t.as_str()) == Some("image_url"))
            })
            .unwrap_or(false)
    })
}

async fn handle_chat_completions(
    mut req: Request,
    env: &Env,
    ctx: &Context,
    user_id: &str,
) -> Result<Response> {
    let body: serde_json::Value = req.json().await?;

    // МАРШРУТИЗАЦИЯ ПО МОДЕЛИ. Всё, что не Workers AI (`@cf/…`), уходит по HTTP к
    // стороннему провайдеру: тот же OpenAI-совместимый протокол, только адрес и
    // ключ берутся из THIRDPARTY_API_URL / THIRDPARTY_API_KEY. Ключ живёт ТОЛЬКО
    // здесь — клиенту он не виден, гейт подписки уже пройден выше.
    if let Some(model) = body.get("model").and_then(|m| m.as_str()) {
        if !is_workers_ai(model) {
            let model = model.to_string();
            return proxy_thirdparty(body, env, ctx, user_id, &model).await;
        }
    }

    // Clone the messages array we will (possibly) massage.
    let mut messages: Vec<serde_json::Value> = body
        .get("messages")
        .and_then(|m| m.as_array())
        .cloned()
        .unwrap_or_default();

    // json_schema response_format → inline + append a JSON-only instruction.
    let schema_opt = body
        .get("response_format")
        .and_then(|rf| rf.get("json_schema"))
        .and_then(|js| js.get("schema"))
        .cloned();
    // Схема, уже развёрнутая (без `$ref`) — уходит и в промпт инструкцией, и в саму
    // Workers AI как `response_format`.
    let inlined_schema = schema_opt.as_ref().map(inline_schema);
    if let Some(schema) = schema_opt {
        let inlined = strip_schema_meta(&inline_schema(&schema));
        let schema_json = serde_json::to_string(&inlined)
            .map_err(|e| Error::RustError(format!("serialize schema: {e}")))?;
        let json_instruction = format!(
            "\n\nYou MUST respond with ONLY valid JSON (no markdown, no explanation, no code fences). \
The JSON MUST conform to this exact schema:\n{schema_json}"
        );
        // Append to the first system message whose content is a string; else unshift.
        let sys_idx = messages.iter().position(|m| {
            m.get("role").and_then(|r| r.as_str()) == Some("system")
        });
        let appended = sys_idx
            .and_then(|i| {
                messages[i]
                    .get("content")
                    .and_then(|c| c.as_str())
                    .map(|s| (i, s.to_string()))
            })
            .map(|(i, content)| {
                let mut m = messages[i].as_object().cloned().unwrap_or_default();
                m.insert(
                    "content".to_string(),
                    serde_json::Value::String(format!("{content}{json_instruction}")),
                );
                messages[i] = serde_json::Value::Object(m);
                true
            })
            .unwrap_or(false);
        if !appended {
            messages.insert(
                0,
                serde_json::json!({
                    "role": "system",
                    "content": format!("You are a helpful assistant.{json_instruction}"),
                }),
            );
        }
    }

    let images = has_image_content(&messages);
    let want_stream = body
        .get("stream")
        .and_then(|s| s.as_bool())
        .unwrap_or(true);

    // Build run params.
    let mut run_params = serde_json::Map::new();
    run_params.insert(
        "messages".to_string(),
        serde_json::Value::Array(messages),
    );
    run_params.insert("stream".to_string(), serde_json::Value::Bool(want_stream));

    // Ask Workers AI to emit a final usage chunk so we can account TEXT tokens
    // backend-authoritatively. NOTE: include_usage support is uncertain on the
    // Workers AI platform — if no usage chunk arrives we report nothing (never guess).
    if want_stream {
        run_params.insert(
            "stream_options".to_string(),
            serde_json::json!({ "include_usage": true }),
        );
    }

    // Reasoning control. A client may override explicitly via chat_template_kwargs;
    // else if NO image, honour the top-level `think` flag (arti-pipes sends it) —
    // default ON, but a client that sets `think:false` gets thinking OFF. This
    // matters because qwen3 with thinking sometimes emits ALL of a short answer into
    // the reasoning channel and NOTHING into content (observed ~⅔ of the time for
    // some foods), which surfaces as an empty response; thinking OFF makes the model
    // put the answer in content reliably. Image requests pass no kwargs.
    if let Some(ctk) = body.get("chat_template_kwargs") {
        run_params.insert("chat_template_kwargs".to_string(), ctk.clone());
    } else if !images {
        let enable_thinking = body.get("think").and_then(|t| t.as_bool()).unwrap_or(true);
        run_params.insert(
            "chat_template_kwargs".to_string(),
            serde_json::json!({ "enable_thinking": enable_thinking }),
        );
    }

    // СХЕМА УХОДИТ И В САМУ WORKERS AI, а не только текстом в промпт.
    //
    // Инструкция в промпте — просьба, и модель её изредка нарушает: замер поймал, что
    // ответ доходит целиком (finish_reason=stop, [DONE], usage на месте), но в конце
    // стоит лишняя закрывающая скобка — `…"}}]}` вместо `…"}]}`. Разбор такого падает,
    // и попытка пропадает целиком. Это не обрыв связи и не потолок токенов, это
    // невалидный JSON, который никто не обязывался делать валидным.
    //
    // `response_format` включает на стороне платформы декодирование ПО ГРАММАТИКЕ —
    // лишней скобке там взяться неоткуда. Инструкция в промпте остаётся: она задаёт
    // смысл полей, а грамматика — форму.
    if let Some(schema) = inlined_schema {
        run_params.insert(
            "response_format".to_string(),
            serde_json::json!({ "type": "json_schema", "json_schema": schema }),
        );
    }

    // Forward the client's max_tokens when it is a number.
    if let Some(mt) = body.get("max_tokens") {
        if mt.is_number() {
            run_params.insert("max_tokens".to_string(), mt.clone());
        }
    }

    let model = body
        .get("model")
        .and_then(|m| m.as_str())
        .ok_or_else(|| Error::RustError("missing model".into()))?
        .to_string();

    let run_params = serde_json::Value::Object(run_params);
    let out = ai_run(env, &model, &run_params).await?;

    if !want_stream {
        let out_val: serde_json::Value = serde_wasm_bindgen::from_value(out)
            .map_err(|e| Error::RustError(format!("AI.run output decode: {e}")))?;
        // Best-effort TEXT token/neuron accounting (never blocks/affects the response).
        if let Some((p, c)) = usage_split(&out_val) {
            let env = env.clone();
            let user_id = user_id.to_string();
            let model = model.clone();
            ctx.wait_until(async move { report_usage(&env, &user_id, &model, p, c).await });
        }
        return Response::from_json(&out_val);
    }

    // Raw passthrough of the Workers AI SSE byte stream — no re-parsing, no copying.
    //
    // This used to `tee()` the stream: branch 0 to the client, branch 1 drained at
    // full speed inside `ctx.wait_until` to pick out the `usage` chunk. Two consumers
    // of one source means the split has to buffer for whichever reads slower, and the
    // client — reading over the network — is always the slower one. Measured on dev:
    // answers longer than ~55 SSE frames died mid-token in roughly half the requests,
    // with no `[DONE]` and no `finish_reason` — the body simply ended. Short answers
    // (calcium, ~53 frames) finished before that point and looked fine, which is why
    // this stayed hidden until the iron lookup made answers longer.
    //
    // Now there is ONE consumer. The bytes go to the client untouched and are merely
    // WATCHED on the way past: complete SSE lines are scanned for the usage chunk, and
    // the finding is handed to `ctx.wait_until` through a channel. Nothing is
    // duplicated, so nothing has to be buffered for a second reader.
    let stream = worker::web_sys::ReadableStream::unchecked_from_js(out);

    // Workers AI puts a `usage` field on EVERY chunk, not just the last one: the first
    // carries (prompt, 0), the middle ones (0, 1) — one completion token each — and
    // only the FINAL chunk carries the totals (prompt, completion). So the LAST usage
    // line is the answer; stopping at the first one records a zero completion count.
    // Hence an mpsc channel rather than a oneshot: every sighting is sent, and the
    // reporter keeps the last one it received before the stream ended.
    let (tx, mut rx) = futures_channel::mpsc::unbounded::<(i64, i64)>();
    {
        let env = env.clone();
        let user_id = user_id.to_string();
        let model = model.clone();
        ctx.wait_until(async move {
            let mut last: Option<(i64, i64)> = None;
            while let Some(u) = futures_util::StreamExt::next(&mut rx).await {
                last = Some(u);
            }
            match last {
                Some((p, c)) => report_usage(&env, &user_id, &model, p, c).await,
                // The sender dropped without a single sighting: the stream ended (or the
                // client left) with no usage chunk. Report nothing — never guess.
                None => console_error!(
                    "usage-report: no usage chunk in stream (include_usage may be unsupported); reporting nothing"
                ),
            }
        });
    }

    // Состояние наблюдателя: недособранный хвост строки, счётчик подряд идущих `!`
    // и признак того, что поток уже оборван предохранителем.
    struct Watch {
        carry: Vec<u8>,
        bangs: usize,
        aborted: bool,
    }
    let watch = Watch { carry: Vec::new(), bangs: 0, aborted: false };

    // ПРЕДОХРАНИТЕЛЬ. Модель изредка срывается и молотит один символ до самого
    // потолка — наблюдалось 6774 знака `!` подряд на запрос омега-3. Разобрать это
    // нельзя, попытка всё равно пропадёт, но токены тарифицируются все. Поэтому
    // поток обрывается на месте: клиент получает явную ошибку, а генерация
    // прекращается — мы перестаём читать тело, и запрос к Workers AI отменяется.
    let watched = futures_util::StreamExt::scan(
        worker::ByteStream::from(stream),
        watch,
        move |st, chunk: Result<Vec<u8>>| {
            // Хвост уже отправлен — поток закончен.
            if st.aborted {
                return futures_util::future::ready(None);
            }
            if let Ok(bytes) = &chunk {
                st.carry.extend_from_slice(bytes);
                // Only COMPLETE lines can be parsed; the tail waits for the next chunk.
                let mut consumed = 0usize;
                let mut runaway = false;
                for line in st.carry.split_inclusive(|b| *b == b'\n') {
                    if !line.ends_with(b"\n") {
                        break;
                    }
                    consumed += line.len();
                    let text = String::from_utf8_lossy(line);
                    if let Some(u) = usage_from_sse_line(&text) {
                        let _ = tx.unbounded_send(u);
                    }
                    // Счётчик ведётся по СКЛЕЕННОМУ ответу, а не по сырым байтам:
                    // при срыве каждый знак приезжает отдельным чанком, и подряд в
                    // одном чанке они никогда не встретятся.
                    if let Some(c) = content_from_sse_line(&text) {
                        for ch in c.chars() {
                            st.bangs = if ch == '!' { st.bangs + 1 } else { 0 };
                            if st.bangs >= RUNAWAY_BANGS {
                                runaway = true;
                                break;
                            }
                        }
                    }
                    if runaway {
                        break;
                    }
                }
                st.carry.drain(..consumed.min(st.carry.len()));
                // A single SSE line is small; anything longer than this is not a line
                // we are waiting for, and the buffer must not grow without bound.
                if st.carry.len() > 64 * 1024 {
                    st.carry.clear();
                }
                if runaway {
                    st.aborted = true;
                    console_error!(
                        "runaway generation: {RUNAWAY_BANGS} consecutive '!' — stream aborted"
                    );
                    return futures_util::future::ready(Some(Ok(runaway_sse_tail())));
                }
            }
            futures_util::future::ready(Some(chunk))
        },
    );
    let resp = Response::from_stream(watched)?;
    let headers = resp.headers();
    headers.set("Content-Type", "text/event-stream")?;
    headers.set("Cache-Control", "no-cache")?;
    Ok(resp)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn registry() -> Vec<Provider> {
        serde_json::from_str(
            r#"[
                {"models":["alpha-vision","alpha-mini"],"url":"https://alpha/v1/chat/completions","key":"ALPHA_KEY"},
                {"models":["beta-vision"],"url":"https://beta/v1/chat/completions","key":"BETA_KEY"}
            ]"#,
        )
        .expect("реестр разбирается")
    }

    #[test]
    fn modeli_raskidyvayutsya_po_svoim_klyucham() {
        let providers = registry();
        let a = pick_provider(&providers, "alpha-mini").expect("альфа найдена");
        assert_eq!(a.key, "ALPHA_KEY");
        let b = pick_provider(&providers, "beta-vision").expect("бета найдена");
        assert_eq!(b.key, "BETA_KEY");
        assert_eq!(b.url, "https://beta/v1/chat/completions");
    }

    #[test]
    fn neizvestnaya_model_bez_zvezdochki_ne_marshrutiziruetsya() {
        assert!(pick_provider(&registry(), "gamma-vision").is_none());
    }

    #[test]
    fn zvezdochka_beryotsya_tolko_posle_poimyonnyh() {
        let providers: Vec<Provider> = serde_json::from_str(
            r#"[
                {"models":["*"],"url":"https://any/v1/chat/completions","key":"ANY_KEY"},
                {"models":["alpha-vision"],"url":"https://alpha/v1/chat/completions","key":"ALPHA_KEY"}
            ]"#,
        )
        .expect("реестр разбирается");
        assert_eq!(pick_provider(&providers, "alpha-vision").unwrap().key, "ALPHA_KEY");
        assert_eq!(pick_provider(&providers, "что-угодно").unwrap().key, "ANY_KEY");
    }

    #[test]
    fn modeli_workers_ai_ne_uhodyat_naruzhu() {
        assert!(is_workers_ai("@cf/qwen/qwen3-30b-a3b-fp8"));
        assert!(!is_workers_ai("alpha-vision"));
    }
}

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
        report_usage_inner(env, &key, user_id, prompt, completion, in_neurons, out_neurons).await
    {
        console_error!("usage-report failed (best-effort, swallowed): {e:?}");
    }
}

#[allow(clippy::too_many_arguments)]
async fn report_usage_inner(
    env: &Env,
    key: &str,
    user_id: &str,
    in_tokens: i64,
    out_tokens: i64,
    in_neurons: i64,
    out_neurons: i64,
) -> Result<()> {
    let headers = Headers::new();
    headers.set("X-Internal-Key", key)?;
    headers.set("Content-Type", "application/json")?;
    let body = serde_json::json!({
        "userId": user_id, "source": "text",
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

/// Read a TEE'd copy of the SSE byte stream fully, scan `data:` lines for the chunk
/// carrying a top-level `usage`, and report it. If no usage chunk arrives (the
/// platform may not support `include_usage`), log and report nothing — never guess.
async fn report_stream_usage(
    env: &Env,
    user_id: String,
    model: String,
    branch: worker::web_sys::ReadableStream,
) {
    let bytes = match drain_stream(&branch).await {
        Ok(b) => b,
        Err(e) => {
            console_error!("usage-report: draining tee'd stream failed (swallowed): {e:?}");
            return;
        }
    };
    let text = String::from_utf8_lossy(&bytes);
    let mut found: Option<(i64, i64)> = None;
    for line in text.lines() {
        let data = match line.strip_prefix("data:") {
            Some(d) => d.trim(),
            None => continue,
        };
        if data.is_empty() || data == "[DONE]" {
            continue;
        }
        if let Ok(val) = serde_json::from_str::<serde_json::Value>(data) {
            if let Some(split) = usage_split(&val) {
                found = Some(split);
            }
        }
    }
    match found {
        Some((p, c)) => report_usage(env, &user_id, &model, p, c).await,
        None => console_error!(
            "usage-report: no usage chunk in stream (include_usage may be unsupported); reporting nothing"
        ),
    }
}

/// Fully read a web_sys ReadableStream into bytes via its default reader.
async fn drain_stream(stream: &worker::web_sys::ReadableStream) -> Result<Vec<u8>> {
    use wasm_bindgen::JsValue;
    use worker::wasm_bindgen_futures::JsFuture;
    let reader = stream
        .get_reader()
        .dyn_into::<worker::web_sys::ReadableStreamDefaultReader>()
        .map_err(|_| Error::RustError("stream reader is not a default reader".into()))?;
    let mut out = Vec::new();
    loop {
        let result = JsFuture::from(reader.read())
            .await
            .map_err(|e| Error::RustError(format!("stream read: {e:?}")))?;
        let done = js_sys::Reflect::get(&result, &JsValue::from_str("done"))
            .ok()
            .and_then(|v| v.as_bool())
            .unwrap_or(true);
        if done {
            break;
        }
        let value = js_sys::Reflect::get(&result, &JsValue::from_str("value"))
            .map_err(|e| Error::RustError(format!("stream chunk value: {e:?}")))?;
        let chunk = js_sys::Uint8Array::new(&value);
        out.extend_from_slice(&chunk.to_vec());
    }
    Ok(out)
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
    if let Some(schema) = schema_opt {
        let inlined = inline_schema(&schema);
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

    // Raw passthrough of the Workers AI SSE byte stream — no re-parsing. `stream:true`
    // resolves to a ReadableStream. TEE it: branch 0 → the client byte-for-byte;
    // branch 1 → drained inside ctx.wait_until to extract the usage chunk (if any).
    let stream = worker::web_sys::ReadableStream::unchecked_from_js(out);
    let tee = stream.tee();
    let client_branch = worker::web_sys::ReadableStream::unchecked_from_js(tee.get(0));
    let usage_branch = worker::web_sys::ReadableStream::unchecked_from_js(tee.get(1));
    {
        let env = env.clone();
        let user_id = user_id.to_string();
        let model = model.clone();
        ctx.wait_until(async move { report_stream_usage(&env, user_id, model, usage_branch).await });
    }
    let resp = Response::from_stream(worker::ByteStream::from(client_branch))?;
    let headers = resp.headers();
    headers.set("Content-Type", "text/event-stream")?;
    headers.set("Cache-Control", "no-cache")?;
    Ok(resp)
}

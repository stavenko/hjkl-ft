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

use worker::*;

mod token;
mod types;

use token::{secret_or_var, validate_from_header};

// ── CORS ────────────────────────────────────────────────────────────────────
// Known origins only (no wildcard): the prod app + any renorma.app subdomain,
// the dev test env, and localhost/127.0.0.1 for development. Mirrors the TS
// ALLOWED_ORIGIN_RE regex.
fn is_allowed_origin(origin: &str) -> bool {
    origin == "https://renorma.app"
        || (origin.starts_with("https://") && origin.ends_with(".renorma.app"))
        || origin == "https://hjkl-ft.pages.dev"
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

/// True if the user's paid subscription is still active. Reads the per-user
/// SubscriptionDO owned by payment-worker. There is no trial: a never-paid
/// account reports active:false until it claims a paid guest subscription.
async fn subscription_active(env: &Env, user_id: &str) -> Result<bool> {
    let stub = env
        .durable_object("SUBSCRIPTION_DO")?
        .id_from_name(user_id)?
        .get_stub()?;
    let mut res = stub.fetch_with_str("https://do/subscription").await?;
    if res.status_code() < 200 || res.status_code() >= 300 {
        return Ok(false);
    }
    let status: serde_json::Value = res.json().await?;
    Ok(status.get("active").and_then(|v| v.as_bool()) == Some(true))
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

    let resp = match handle(req, &env).await {
        Ok(r) => r,
        Err(e) => error_response(&e.to_string(), 500),
    };
    add_cors(resp, &origin)
}

async fn handle(req: Request, env: &Env) -> Result<Response> {
    // JWT verify (Authorization: Bearer). 401 on missing/invalid.
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
        // Gate AI on an active subscription (Trial not expired, or Paid).
        if !subscription_active(env, &user_id).await? {
            return Ok(error_response("subscription_required", 402));
        }
        return handle_chat_completions(req, env).await;
    }

    Ok(error_response("Not found", 404))
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

async fn handle_chat_completions(mut req: Request, env: &Env) -> Result<Response> {
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

    // Reasoning control. A client may override explicitly via chat_template_kwargs;
    // else if NO image, enable thinking; else (image present) pass none.
    if let Some(ctk) = body.get("chat_template_kwargs") {
        run_params.insert("chat_template_kwargs".to_string(), ctk.clone());
    } else if !images {
        run_params.insert(
            "chat_template_kwargs".to_string(),
            serde_json::json!({ "enable_thinking": true }),
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
    let ai = env.ai("AI")?;

    if !want_stream {
        let out: serde_json::Value = ai.run(&model, &run_params).await?;
        return Response::from_json(&out);
    }

    // Raw passthrough of the Workers AI SSE byte stream — no re-parsing.
    let stream = ai.run_bytes(&model, &run_params).await?;
    let resp = Response::from_stream(stream)?;
    let headers = resp.headers();
    headers.set("Content-Type", "text/event-stream")?;
    headers.set("Cache-Control", "no-cache")?;
    Ok(resp)
}

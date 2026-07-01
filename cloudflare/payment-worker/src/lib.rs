// Subscriptions + real payments (provider-agnostic; lava.top first).
//
// The per-user SubscriptionDO is the single source of truth that every gate reads
// (ai-worker / ocr-queue: GET /subscription → {active}). Payment providers only
// drive its state via webhooks. There is NO trial: a never-paid account has end=0
// → active:false; access becomes true only by claiming a paid guest subscription.
// PaymentIndexDO maps a provider's orderId / contractId back to our user id (or a
// guest claimId). ClaimDO is the guest paid-sub ledger and the atomic claim CAS.
//
// /admin/* authorizes through the SUPPORT_WORKER service binding + INTERNAL_PUSH_KEY
// against the support-worker approved-admins (one source of truth; no allowlist).

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use sha2::{Digest, Sha256};
use wasm_bindgen::JsValue;
use worker::*;

mod claim_do;
mod payment_index_do;
mod providers;
mod subscription_do;
mod token;
mod types;

pub use claim_do::ClaimDO;
pub use payment_index_do::PaymentIndexDO;
pub use subscription_do::SubscriptionDO;

use providers::{provider_for, CheckoutOpts, Lava, WebhookEvent, WebhookKind};
use token::validate_from_header;

// ── CORS ────────────────────────────────────────────────────────────────────
// Known origins only (no wildcard): the prod app + any renorma.app subdomain, the
// dev test env, and localhost for development.
fn is_allowed_origin(origin: &str) -> bool {
    origin == "https://renorma.app"
        || (origin.starts_with("https://") && origin.ends_with(".renorma.app"))
        || origin == "https://renorma-fit-dev.pages.dev"
        || origin == "https://renorma-admin-dev.pages.dev"
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

// ── error helpers ─────────────────────────────────────────────────────────────
fn error_response(message: &str, status: u16) -> Response {
    Response::from_json(&serde_json::json!({ "error": message }))
        .expect("serialize error")
        .with_status(status)
}

// ── DO-stub helpers ───────────────────────────────────────────────────────────
// Storage epoch: BUMP this to wipe ALL payment DO state in a single deploy. The
// worker starts addressing fresh (empty) DO instances by name; the old ones simply
// orphan. This avoids delete-class migrations (Cloudflare rejects those while the
// binding still references the class). Increment again for the next reset.
const DO_EPOCH: &str = "v2";

fn sub_stub(env: &Env, user_id: &str) -> Result<worker::durable::Stub> {
    env.durable_object("SUBSCRIPTION_DO")?
        .id_from_name(&format!("{DO_EPOCH}:{user_id}"))?
        .get_stub()
}
fn index_stub(env: &Env) -> Result<worker::durable::Stub> {
    env.durable_object("PAYMENT_INDEX_DO")?
        .id_from_name(&format!("index-{DO_EPOCH}"))?
        .get_stub()
}
fn claim_stub(env: &Env) -> Result<worker::durable::Stub> {
    env.durable_object("CLAIM_DO")?
        .id_from_name(&format!("claims-{DO_EPOCH}"))?
        .get_stub()
}

/// POST to a DO stub at `https://do{path}` with a JSON body. Returns the raw Response.
async fn do_post(
    stub: &worker::durable::Stub,
    path: &str,
    body: &serde_json::Value,
) -> Result<Response> {
    let url = format!("https://do{path}");
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
    let req = Request::new_with_init(&url, &init)?;
    stub.fetch_with_request(req).await
}

async fn do_get(stub: &worker::durable::Stub, path: &str) -> Result<Response> {
    stub.fetch_with_str(&format!("https://do{path}")).await
}

// Index convenience wrappers.
async fn index_get(env: &Env, key: &str) -> Result<Option<String>> {
    let stub = index_stub(env)?;
    let mut res = do_get(
        &stub,
        &format!("/get?key={}", js_sys::encode_uri_component(key).as_string().unwrap_or_default()),
    )
    .await?;
    let v: serde_json::Value = res.json().await?;
    Ok(v.get("userId").and_then(|u| u.as_str()).map(String::from))
}
async fn index_put(env: &Env, key: &str, user_id: &str) -> Result<()> {
    let stub = index_stub(env)?;
    do_post(&stub, "/put", &serde_json::json!({ "key": key, "userId": user_id })).await?;
    Ok(())
}
async fn index_delete(env: &Env, key: &str) -> Result<()> {
    let stub = index_stub(env)?;
    do_post(&stub, "/delete", &serde_json::json!({ "key": key })).await?;
    Ok(())
}

/// PRODUCTION-IMPOSSIBLE test entitlement: true only when TEST_ENTITLEMENT == "1".
/// Absent in [env.production.vars] → false in prod (no free-sub backdoor).
fn test_entitlement_on(env: &Env) -> bool {
    env.var("TEST_ENTITLEMENT")
        .map(|v| v.to_string())
        .map(|v| v == "1")
        .unwrap_or(false)
}

// ── plan catalog ──────────────────────────────────────────────────────────────
fn plans(env: &Env) -> Vec<serde_json::Value> {
    let raw = match env.var("PLANS") {
        Ok(v) => v.to_string(),
        Err(_) => return vec![],
    };
    serde_json::from_str::<Vec<serde_json::Value>>(&raw).unwrap_or_default()
}
/// Strip the server-side-only `lavaOfferId` for the public /plans listing.
fn public_plan(p: &serde_json::Value) -> serde_json::Value {
    let mut out = p.clone();
    if let Some(obj) = out.as_object_mut() {
        obj.remove("lavaOfferId");
    }
    out
}

// ── claim-secret crypto (MONEY-SAFETY #1) ─────────────────────────────────────
/// 256-bit (>=128-bit) random secret, base64url, no padding. Used both for the
/// opaque public claimId AND the high-entropy claim secret.
fn random_claim_secret() -> Result<String> {
    let mut bytes = [0u8; 32];
    getrandom::getrandom(&mut bytes).map_err(|e| Error::RustError(format!("getrandom: {e}")))?;
    Ok(URL_SAFE_NO_PAD.encode(bytes))
}

/// Lowercase hex sha256 — the DB stores ONLY hash(secret), never plaintext nor the
/// lava contractId.
fn sha256_hex(s: &str) -> String {
    let digest = Sha256::digest(s.as_bytes());
    let mut out = String::with_capacity(64);
    for b in digest {
        out.push_str(&format!("{b:02x}"));
    }
    out
}

/// Stable webhook dedup key (MONEY-SAFETY #4). Prefer a provider event id; else
/// compose from kind + contract + provider timestamp. Stable across retries.
fn event_key(name: &str, ev: &WebhookEvent, raw: &serde_json::Value) -> String {
    if let Some(id) = &ev.event_id {
        return format!("{name}:{id}");
    }
    let contract = ev
        .contract_id
        .clone()
        .or_else(|| ev.parent_contract_id.clone())
        .unwrap_or_default();
    let ts = ev
        .timestamp
        .clone()
        .or_else(|| raw.get("timestamp").and_then(|v| v.as_str()).map(String::from))
        .or_else(|| raw.get("eventTime").and_then(|v| v.as_str()).map(String::from))
        .unwrap_or_default();
    format!("{name}:{}:{contract}:{ts}", kind_str(&ev.kind))
}

fn kind_str(k: &WebhookKind) -> &'static str {
    match k {
        WebhookKind::Paid => "paid",
        WebhookKind::Recurring => "recurring",
        WebhookKind::Cancelled => "cancelled",
        WebhookKind::Refunded => "refunded",
        WebhookKind::Failed => "failed",
    }
}

// ── provider credential resolution (Secrets Store; dev/test = None) ────────────
/// Resolve a LAVA credential from the Secrets Store, distinguishing "no binding"
/// (dev/test → Ok(None), legitimately absent) from "binding present but
/// unresolvable" (prod misconfig → Err, FAIL LOUDLY — never swallow per CLAUDE.md).
async fn read_secret_store(env: &Env, binding: &str) -> std::result::Result<Option<String>, String> {
    match env.secret_store(binding) {
        // Binding EXISTS (prod): the value MUST resolve, else loud misconfig.
        Ok(store) => match store.get().await {
            Ok(Some(v)) if !v.is_empty() => Ok(Some(v)),
            Ok(_) => Err(format!("MISCONFIGURED: Secrets Store binding '{binding}' is empty/unset")),
            Err(e) => Err(format!("MISCONFIGURED: Secrets Store binding '{binding}' get() failed: {e:?}")),
        },
        // No binding (dev/test) → None → provider legitimately not configured.
        Err(_) => Ok(None),
    }
}

/// Build a provider with credentials resolved from the Secrets Store. dev/test bind
/// NO lava store → both creds Ok(None) → not configured → real pay impossible.
/// A present-but-unresolvable LAVA binding (prod misconfig) propagates Err loudly.
async fn provider_for_env(name: &str, env: &Env) -> std::result::Result<Option<Lava>, String> {
    let api_key = read_secret_store(env, "LAVA_API_KEY").await?;
    let webhook_secret = read_secret_store(env, "LAVA_WEBHOOK_SECRET").await?;
    Ok(provider_for(name, api_key, webhook_secret))
}

// ── push (best-effort) ────────────────────────────────────────────────────────
/// "payment succeeded" push via main-flow's /push/notify (plain URL fetch, shared
/// INTERNAL_PUSH_KEY). NEVER fails the webhook (payment already succeeded) — but a
/// failure is logged loudly, never swallowed silently.
async fn notify_push(env: &Env, user_id: &str, body: &str, url_path: &str) {
    let base = env.var("PUSH_NOTIFY_URL").map(|v| v.to_string()).ok();
    let key = token::secret_or_var(env, "INTERNAL_PUSH_KEY").await.ok();
    let (base, key) = match (base, key) {
        (Some(b), Some(k)) if !b.is_empty() && !k.is_empty() => (b, k),
        _ => {
            console_warn!(
                "notifyPush: PUSH_NOTIFY_URL / INTERNAL_PUSH_KEY not configured — skipping push"
            );
            return;
        }
    };
    let payload = serde_json::json!({ "userId": user_id, "body": body, "url": url_path }).to_string();
    let headers = Headers::new();
    let _ = headers.set("Content-Type", "application/json");
    let _ = headers.set("X-Internal-Key", &key);
    let mut init = RequestInit::new();
    init.with_method(Method::Post)
        .with_headers(headers)
        .with_body(Some(JsValue::from_str(&payload)));
    let req = match Request::new_with_init(&base, &init) {
        Ok(r) => r,
        Err(e) => {
            console_error!("notifyPush build request failed: {e}");
            return;
        }
    };
    match Fetch::Request(req).send().await {
        Ok(mut res) => {
            let status = res.status_code();
            if !(200..300).contains(&status) {
                let txt = res.text().await.unwrap_or_default();
                console_error!("notifyPush: {status} {txt}");
            }
        }
        Err(e) => console_error!("notifyPush failed: {e}"),
    }
}

/// "guest payment succeeded" → notify telegram-worker so it can send the user the
/// claim-binding link. Best-effort: mirrors `notify_push` resilience — logs on every
/// failure and NEVER fails the webhook (lava must always get its 200). Over the
/// TELEGRAM_WORKER service binding, guarded by the shared INTERNAL_PUSH_KEY. The
/// claimId is the opaque public id (not secret-bearing) so logging it is acceptable;
/// the claim secret is not in scope here.
async fn notify_telegram_paid(env: &Env, claim_id: &str) {
    let key = match token::secret_or_var(env, "INTERNAL_PUSH_KEY").await {
        Ok(k) if !k.is_empty() => k,
        _ => {
            console_warn!("notifyTelegramPaid: INTERNAL_PUSH_KEY not configured — skipping");
            return;
        }
    };
    let payload = serde_json::json!({ "claimId": claim_id }).to_string();
    let headers = Headers::new();
    let _ = headers.set("Content-Type", "application/json");
    let _ = headers.set("X-Internal-Key", &key);
    let mut init = RequestInit::new();
    init.with_method(Method::Post)
        .with_headers(headers)
        .with_body(Some(JsValue::from_str(&payload)));
    // Host is irrelevant for a service-binding fetch; only the path routes.
    let request = match Request::new_with_init("https://telegram-worker/internal/paid", &init) {
        Ok(r) => r,
        Err(e) => {
            console_error!("notifyTelegramPaid build request failed: {e}");
            return;
        }
    };
    let tg = match env.service("TELEGRAM_WORKER") {
        Ok(s) => s,
        Err(e) => {
            console_error!("notifyTelegramPaid: TELEGRAM_WORKER binding error: {e}");
            return;
        }
    };
    match tg.fetch_request(request).await {
        Ok(mut res) => {
            let status = res.status_code();
            if !(200..300).contains(&status) {
                let txt = res.text().await.unwrap_or_default();
                console_error!("notifyTelegramPaid: {status} {txt} claimId={claim_id}");
            }
        }
        Err(e) => console_error!("notifyTelegramPaid failed claimId={claim_id}: {e}"),
    }
}

// ── unified admin auth (SUPPORT_WORKER binding + INTERNAL_PUSH_KEY) ────────────
/// Authorize the caller as an approved admin. Verifies the expert JWT (same
/// JWT_SECRET / auth-worker) → sub → asks support-worker /internal/is-admin via the
/// service binding. One source of truth (the support-worker approved-admins); no
/// env allowlist, no redeploy to add an admin. Fails CLOSED on every error.
async fn require_admin(req: &Request, env: &Env) -> std::result::Result<String, Response> {
    let sub = validate_from_header(req, env)
        .await
        .map_err(|_| error_response("Unauthorized", 401))?;

    let key = match token::secret_or_var(env, "INTERNAL_PUSH_KEY").await {
        Ok(k) => k,
        Err(_) => return Err(error_response("admin_not_configured", 500)),
    };

    let body = serde_json::json!({ "sub": sub }).to_string();
    let headers = Headers::new();
    let _ = headers.set("Content-Type", "application/json");
    let _ = headers.set("X-Internal-Key", &key);
    let mut init = RequestInit::new();
    init.with_method(Method::Post)
        .with_headers(headers)
        .with_body(Some(JsValue::from_str(&body)));
    // The host is irrelevant for a service-binding fetch; only the path routes.
    let request = match Request::new_with_init("https://support-worker/internal/is-admin", &init) {
        Ok(r) => r,
        Err(_) => return Err(error_response("admin_auth_error", 500)),
    };
    let support = match env.service("SUPPORT_WORKER") {
        Ok(s) => s,
        Err(_) => return Err(error_response("admin_auth_binding", 500)),
    };
    let mut resp = match support.fetch_request(request).await {
        Ok(r) => r,
        Err(_) => return Err(error_response("admin_auth_fetch", 500)),
    };
    if resp.status_code() != 200 {
        return Err(error_response("admin_auth_error", 500));
    }
    let v: serde_json::Value = match resp.json().await {
        Ok(v) => v,
        Err(_) => return Err(error_response("admin_auth_parse", 500)),
    };
    if v.get("approved").and_then(|b| b.as_bool()).unwrap_or(false) {
        Ok(sub)
    } else {
        Err(error_response("forbidden", 403))
    }
}

// ── relay a DO response (body + status) verbatim ──────────────────────────────
async fn relay(mut res: Response) -> Result<Response> {
    let status = res.status_code();
    let text = res.text().await?;
    let headers = Headers::new();
    let _ = headers.set("Content-Type", "application/json");
    Ok(Response::ok(text)?.with_status(status).with_headers(headers))
}

/// Unbound (paid-but-unclaimed) payments, RECONCILED against lava. lava has no refund
/// webhook, but its GET /api/v2/invoices exposes `subscriptionDetails.terminatedAt` /
/// `subscriptionStatus=CANCELLED` — so a refunded/cancelled contract is detectable. We
/// AUTO-VOID such claims (tombstone → non-redeemable, MONEY-SAFETY #4/#7) and drop them
/// from the worklist. Degrade gracefully: if lava is unreachable OR a claim's contract
/// isn't in the fetched page, we KEEP the row (never hide an actionable payment on doubt).
async fn admin_unbound_reconciled(env: &Env) -> Result<Response> {
    let stub = claim_stub(env)?;
    let mut r = do_get(&stub, "/unbound").await?;
    let mut body: serde_json::Value = r.json().await?;
    let rows = body
        .get("unbound")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    if rows.is_empty() {
        return Response::from_json(&body);
    }

    // lava contractIds whose access is terminated (refund/cancel). Absent provider (dev)
    // or a lava error → no reconcile: return the raw list unchanged.
    let terminated: std::collections::HashSet<String> = match provider_for_env("lava", env).await {
        Ok(Some(p)) if p.configured() => match p.list_invoices(1, 100).await {
            Ok(page) => {
                let items = page.get("items").and_then(|v| v.as_array());
                let total = page.get("total").and_then(|v| v.as_i64()).unwrap_or(0);
                if let Some(items) = items {
                    if (items.len() as i64) < total {
                        console_warn!(
                            "unbound reconcile: lava has {total} contracts but only {} fetched — claims beyond page 1 not reconciled",
                            items.len()
                        );
                    }
                    items
                        .iter()
                        .filter(|it| is_terminated(it))
                        .filter_map(|it| it.get("id").and_then(|v| v.as_str()).map(String::from))
                        .collect()
                } else {
                    return Response::from_json(&body);
                }
            }
            Err(e) => {
                console_error!("unbound reconcile: lava list_invoices failed: {e}");
                return Response::from_json(&body);
            }
        },
        _ => return Response::from_json(&body),
    };

    let mut kept: Vec<serde_json::Value> = Vec::new();
    for row in rows {
        let contract = row.get("contract_id").and_then(|v| v.as_str()).unwrap_or("");
        let claim_id = row.get("claim_id").and_then(|v| v.as_str()).unwrap_or("");
        if !contract.is_empty() && !claim_id.is_empty() && terminated.contains(contract) {
            // Idempotent tombstone. 409 already_claimed is fine (those aren't unbound);
            // log any other non-2xx loudly (no silent swallow).
            match do_post(&stub, "/void", &serde_json::json!({ "claimId": claim_id })).await {
                Ok(mut resp) => {
                    let sc = resp.status_code();
                    if (200..300).contains(&sc) {
                        console_log!(
                            "unbound reconcile: auto-voided claim {claim_id} (lava contract {contract} terminated)"
                        );
                    } else if sc != 409 {
                        let t = resp.text().await.unwrap_or_default();
                        console_warn!("unbound reconcile: void {claim_id} → {sc}: {t}");
                    }
                }
                Err(e) => console_error!("unbound reconcile: void {claim_id} failed: {e}"),
            }
        } else {
            kept.push(row);
        }
    }
    body["unbound"] = serde_json::Value::Array(kept);
    Response::from_json(&body)
}

/// A lava contract whose access is closed: refunded or the subscription cancelled.
/// (FAILED first-invoices never became `paid` for us, so they don't appear as unbound.)
fn is_terminated(it: &serde_json::Value) -> bool {
    let terminated_at = it
        .get("subscriptionDetails")
        .and_then(|d| d.get("terminatedAt"))
        .and_then(|v| v.as_str());
    if terminated_at.map(|s| !s.is_empty()).unwrap_or(false) {
        return true;
    }
    matches!(
        it.get("subscriptionStatus").and_then(|v| v.as_str()),
        Some("CANCELLED")
    )
}

/// Resolve every REQUIRED Store-bound secret at the top of the fetch entry. On the
/// first failure: log the full reason loudly and return a 503 so ANY request makes
/// the misconfiguration obvious (Workers have no separate startup — per-request is
/// intended). LAVA_* is excluded: it uses the prod-only `read_secret_store` and is
/// legitimately absent in dev.
async fn require_secrets(env: &Env) -> std::result::Result<(), Response> {
    for name in ["JWT_SECRET", "INTERNAL_PUSH_KEY"] {
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

    if req.method() == Method::Options {
        let headers = Headers::new();
        if is_allowed_origin(&origin) {
            let _ = headers.set("Access-Control-Allow-Origin", &origin);
        }
        let _ = headers.set("Vary", "Origin");
        let _ = headers.set("Access-Control-Allow-Methods", "GET, POST, OPTIONS");
        let _ = headers.set("Access-Control-Allow-Headers", "Content-Type, Authorization");
        return Ok(Response::empty()?.with_headers(headers).with_status(204));
    }

    if let Err(resp) = require_secrets(&env).await {
        return Ok(resp);
    }

    let resp = match handle(req, &env).await {
        Ok(r) => r,
        Err(e) => error_response(&e.to_string(), 500),
    };
    add_cors(resp, &origin)
}

async fn handle(req: Request, env: &Env) -> Result<Response> {
    let url = req.url()?;
    let path = url.path().to_string();
    let method = req.method();

    // ── Provider webhooks (NO app JWT — verified by the provider's signature) ──
    if method == Method::Post && path.starts_with("/webhook/") {
        return webhook(req, env, &path["/webhook/".len()..].to_string()).await;
    }

    // ── Guest checkout (NO JWT — landing is unauthenticated; PROD-ONLY) ──
    if method == Method::Post && path == "/checkout/guest" {
        return checkout_guest(req, env).await;
    }

    // ── Public claim-status poll (NO JWT) ──
    // The pay page polls this by the (non-secret) claimId to know when the lava
    // webhook has marked the payment paid. Returns only {status}, never the secret.
    if method == Method::Get && path == "/claim/status" {
        let url = req.url()?;
        let claim_id = url
            .query_pairs()
            .find(|(k, _)| k == "claimId")
            .map(|(_, v)| v.into_owned())
            .unwrap_or_default();
        if claim_id.is_empty() {
            return Ok(error_response("missing claimId", 400));
        }
        let claim = claim_stub(env)?;
        return relay(do_post(&claim, "/status", &serde_json::json!({ "claimId": claim_id })).await?).await;
    }

    // ── TEST entitlement (PRODUCTION-IMPOSSIBLE; TEST_ENTITLEMENT-gated) ──
    if method == Method::Post && path == "/test/guest-checkout" {
        return test_guest_checkout(req, env).await;
    }

    // ── Admin (unified approved-admins via SUPPORT_WORKER) ──
    if method == Method::Get && path == "/admin/unbound-payments" {
        if let Err(resp) = require_admin(&req, env).await {
            return Ok(resp);
        }
        return admin_unbound_reconciled(env).await;
    }
    // Client-requested refunds (access already revoked); operator processes each in lava.
    if method == Method::Get && path == "/admin/refunds" {
        if let Err(resp) = require_admin(&req, env).await {
            return Ok(resp);
        }
        let stub = claim_stub(env)?;
        return relay(do_get(&stub, "/refunds").await?).await;
    }
    // Reconcile a Telegram user: given ?tg=<username|id>, return their claim(s) with
    // status (paid? claimed?) and claimed_by. Backs the operator «оплатил / привязал» check.
    if method == Method::Get && path == "/admin/tg-status" {
        if let Err(resp) = require_admin(&req, env).await {
            return Ok(resp);
        }
        let url = req.url()?;
        let tg = url
            .query_pairs()
            .find(|(k, _)| k == "tg")
            .map(|(_, v)| v.into_owned())
            .unwrap_or_default();
        if tg.trim().is_empty() {
            return Ok(error_response("missing_params", 400));
        }
        let stub = claim_stub(env)?;
        let res = do_post(&stub, "/by-tg", &serde_json::json!({ "tg": tg })).await?;
        return relay(res).await;
    }
    // ── Internal guest checkout (INTERNAL_PUSH_KEY-guarded; PROD-ONLY) ──
    // Same as /checkout/guest but ALSO returns the claim secret, because the caller
    // is our trusted telegram-worker (authenticated by INTERNAL_PUSH_KEY). The secret
    // leaves payment-worker ONLY here, never to a public/unauth caller, never logged.
    if method == Method::Post && path == "/internal/checkout" {
        return internal_checkout(req, env).await;
    }

    // ── Everything else is app-JWT authed ──
    let user_id = match validate_from_header(&req, env).await {
        Ok(sub) => sub,
        Err(_) => return Ok(error_response("Unauthorized", 401)),
    };

    if method == Method::Get && path == "/subscription" {
        let stub = sub_stub(env, &user_id)?;
        return relay(do_get(&stub, "/subscription").await?).await;
    }

    if method == Method::Get && path == "/plans" {
        let public: Vec<serde_json::Value> = plans(env).iter().map(public_plan).collect();
        return Response::from_json(&serde_json::json!({ "plans": public }));
    }

    if method == Method::Post && path == "/claim" {
        return claim(req, env, &user_id).await;
    }

    if method == Method::Post && path == "/cancel" {
        return cancel(env, &user_id).await;
    }

    // Refund: preview the prorated amount (no side effects) …
    if method == Method::Post && path == "/refund/preview" {
        return refund_preview(env, &user_id).await;
    }
    // … and the actual request — records it for the operator AND revokes access now.
    if method == Method::Post && path == "/refund/request" {
        return refund_request(env, &user_id).await;
    }

    Ok(error_response("Not found", 404))
}


// ── shared guest-checkout body ────────────────────────────────────────────────
/// Result of a successful guest checkout. The secret is high-entropy and travels
/// out ONLY via the lava return fragment (public /checkout/guest) or to our trusted
/// telegram-worker (internal /internal/checkout); it is NEVER logged.
struct GuestCheckout {
    pay_url: String,
    claim_id: String,
    secret: String,
}

/// The shared body of guest checkout: provider resolution, plan lookup, claim-secret
/// minting, lava checkout creation, ClaimDO /create-pending + contract→claim index.
/// Returns Err(Response) for every error case (with the SAME statuses as before) so
/// both callers surface identical errors; the PROD-ONLY guard stays in each caller.
async fn do_guest_checkout(
    body: &serde_json::Value,
    env: &Env,
) -> std::result::Result<GuestCheckout, Response> {
    let provider_name = body
        .get("provider")
        .and_then(|v| v.as_str())
        .unwrap_or("lava")
        .to_string();
    let provider = match provider_for_env(&provider_name, env).await {
        Ok(p) => p,
        Err(reason) => {
            console_error!("do_guest_checkout: {reason}");
            return Err(error_response_detail("MISCONFIGURED", &reason, 503));
        }
    };
    let provider = match provider {
        Some(p) if p.configured() => p,
        _ => return Err(error_response("provider_not_configured", 400)),
    };
    let plan_id = body.get("planId").and_then(|v| v.as_str()).unwrap_or("");
    let plan = plans(env).into_iter().find(|p| {
        p.get("id").and_then(|v| v.as_str()) == Some(plan_id)
            && p.get("lavaOfferId")
                .and_then(|v| v.as_str())
                .map(|s| !s.is_empty())
                .unwrap_or(false)
    });
    let plan = match plan {
        Some(p) => p,
        None => return Err(error_response("unknown_plan", 400)),
    };
    let offer_id = plan.get("lavaOfferId").and_then(|v| v.as_str()).unwrap_or("").to_string();
    let plan_price = plan.get("price").and_then(|v| v.as_i64());
    let plan_currency = plan.get("currency").and_then(|v| v.as_str()).map(String::from);
    let plan_id_owned = plan.get("id").and_then(|v| v.as_str()).unwrap_or("").to_string();
    // Optional promo code (trimmed; empty → None).
    let promo_code = body
        .get("promoCode")
        .and_then(|v| v.as_str())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
    // Optional Telegram identity (present only for the Mini App flow) — recorded on the
    // claim so an operator can reconcile «who paid / did they bind an account».
    let tg_user_id = body.get("tgUserId").and_then(|v| v.as_i64());
    let tg_username = body
        .get("tgUsername")
        .and_then(|v| v.as_str())
        .map(|s| s.trim_start_matches('@').trim().to_string())
        .filter(|s| !s.is_empty());

    let claim_id = random_claim_secret().map_err(|e| error_response(&e.to_string(), 500))?; // opaque public id (≠ contractId; #1)
    let secret = random_claim_secret().map_err(|e| error_response(&e.to_string(), 500))?; // high-entropy claim secret (256-bit)
    let secret_hash = sha256_hex(&secret);
    let base = env
        .var("LANDING_RETURN_URL")
        .map(|v| v.to_string())
        .unwrap_or_else(|_| "https://fit.renorma.app/onboard".into());
    // FRAGMENT, not query (#1): not sent to the server, not logged.
    let _return_url = format!("{base}#claim={claim_id}.{secret}");

    let checkout = match provider
        .create_checkout(&CheckoutOpts {
            offer_id,
            email: format!("{claim_id}@guest.renorma.app"),
            return_url: _return_url,
            promo_code,
        })
        .await
    {
        Ok(c) => c,
        Err(e) => return Err(error_response(&format!("checkout_failed: {e}"), 502)),
    };

    let claim = match claim_stub(env) {
        Ok(s) => s,
        Err(e) => return Err(error_response(&e.to_string(), 500)),
    };
    let cp = match do_post(
        &claim,
        "/create-pending",
        &serde_json::json!({
            "claimId": claim_id,
            "secretHash": secret_hash,
            "provider": provider_name,
            "planId": plan_id_owned,
            "contractId": checkout.order_id,
            "amount": plan_price,
            "currency": plan_currency,
            "tgUserId": tg_user_id,
            "tgUsername": tg_username,
        }),
    )
    .await
    {
        Ok(r) => r,
        Err(e) => return Err(error_response(&e.to_string(), 500)),
    };
    if cp.status_code() != 200 {
        return Err(error_response("claim create-pending failed", 500));
    }
    // Map contract → claimId so the webhook finds the guest row.
    if let Err(e) = index_put(env, &format!("claim-contract:{}", checkout.order_id), &claim_id).await {
        return Err(error_response(&e.to_string(), 500));
    }

    Ok(GuestCheckout {
        pay_url: checkout.url,
        claim_id,
        secret,
    })
}

// ── POST /checkout/guest (NO JWT; PROD-ONLY) ──────────────────────────────────
async fn checkout_guest(mut req: Request, env: &Env) -> Result<Response> {
    // Mutually exclusive with the test path: an env that mints free test subs must
    // NOT also take real money. On the test env this route looks non-existent (404).
    if test_entitlement_on(env) {
        return Ok(error_response("Not found", 404));
    }
    let body: serde_json::Value = req.json().await.unwrap_or(serde_json::json!({}));
    let gc = match do_guest_checkout(&body, env).await {
        Ok(gc) => gc,
        Err(resp) => return Ok(resp),
    };
    // claimId only — NEVER the secret (it travels back via the lava fragment).
    Response::from_json(&serde_json::json!({ "payUrl": gc.pay_url, "claimId": gc.claim_id }))
}

// ── POST /internal/checkout (INTERNAL_PUSH_KEY-guarded; PROD-ONLY) ────────────
/// Like /checkout/guest but RETURNS the claim secret, because the caller is our
/// trusted telegram-worker (authenticated by INTERNAL_PUSH_KEY). [SECURITY #4/#5]
async fn internal_checkout(mut req: Request, env: &Env) -> Result<Response> {
    // [SECURITY CHECKPOINT #4] internal-key gate FIRST (fail closed).
    let key = match token::secret_or_var(env, "INTERNAL_PUSH_KEY").await {
        Ok(k) => k,
        Err(e) => {
            console_error!("internal_checkout: {e}");
            return Ok(error_response("internal_not_configured", 500));
        }
    };
    let provided = req
        .headers()
        .get("X-Internal-Key")
        .ok()
        .flatten()
        .unwrap_or_default();
    if provided.is_empty() || provided != key {
        return Ok(error_response("bad internal key", 403));
    }

    // PROD-ONLY guard — identical mutual exclusivity with the test path.
    if test_entitlement_on(env) {
        return Ok(error_response("Not found", 404));
    }

    let body: serde_json::Value = req.json().await.unwrap_or(serde_json::json!({}));
    let gc = match do_guest_checkout(&body, env).await {
        Ok(gc) => gc,
        Err(resp) => return Ok(resp),
    };
    // [SECURITY CHECKPOINT #5] secret egress: ONLY here, internal-key gated, to our
    // own telegram-worker. Never logged.
    Response::from_json(&serde_json::json!({
        "payUrl": gc.pay_url,
        "claimId": gc.claim_id,
        "secret": gc.secret,
    }))
}

// ── POST /test/guest-checkout (PRODUCTION-IMPOSSIBLE) ─────────────────────────
async fn test_guest_checkout(mut req: Request, env: &Env) -> Result<Response> {
    if !test_entitlement_on(env) {
        return Ok(error_response("Not found", 404));
    }
    let body: serde_json::Value = req.json().await.unwrap_or(serde_json::json!({}));
    let plan_id = body.get("planId").and_then(|v| v.as_str()).unwrap_or("");
    let all = plans(env);
    let plan = all
        .iter()
        .find(|p| p.get("id").and_then(|v| v.as_str()) == Some(plan_id))
        .or_else(|| all.first());
    let plan = match plan {
        Some(p) => p,
        None => return Ok(error_response("unknown_plan", 400)),
    };
    let plan_id_owned = plan.get("id").and_then(|v| v.as_str()).unwrap_or("").to_string();

    let claim_id = random_claim_secret()?;
    let secret = random_claim_secret()?;
    let secret_hash = sha256_hex(&secret);

    let claim = claim_stub(env)?;
    let res = do_post(
        &claim,
        "/test-activate",
        &serde_json::json!({
            "claimId": claim_id,
            "secretHash": secret_hash,
            "provider": "test",
            "planId": plan_id_owned,
        }),
    )
    .await?;
    if res.status_code() != 200 {
        return Ok(error_response("test_activate_failed", 500));
    }
    // Test-only: we DO return the secret in JSON (there is no lava redirect).
    Response::from_json(&serde_json::json!({ "claimId": claim_id, "secret": secret }))
}

// ── POST /claim (app-JWT) ─────────────────────────────────────────────────────
async fn claim(mut req: Request, env: &Env, user_id: &str) -> Result<Response> {
    let body: serde_json::Value = req.json().await.unwrap_or(serde_json::json!({}));
    let claim_id = body.get("claimId").and_then(|v| v.as_str()).unwrap_or("");
    let secret = body.get("secret").and_then(|v| v.as_str()).unwrap_or("");
    if claim_id.is_empty() || secret.is_empty() {
        return Ok(error_response("missing_params", 400));
    }
    let secret_hash = sha256_hex(secret);

    // ATOMIC compare-and-set inside ClaimDO (MONEY-SAFETY #3).
    let claim = claim_stub(env)?;
    let mut r = do_post(
        &claim,
        "/claim",
        &serde_json::json!({ "claimId": claim_id, "secretHash": secret_hash, "userId": user_id }),
    )
    .await?;
    if r.status_code() != 200 {
        // 404 claim_not_found | 403 bad_secret/claimed_by_other | 409 not_paid_yet/claim_void.
        return relay(r).await;
    }
    let cr: serde_json::Value = r.json().await?;
    let period_end = cr.get("periodEnd").and_then(|v| v.as_i64());
    let provider = cr.get("provider").and_then(|v| v.as_str()).map(String::from);
    let contract_id = cr.get("contractId").and_then(|v| v.as_str()).map(String::from);
    let email = cr.get("email").and_then(|v| v.as_str()).map(String::from);

    // Activate the user's SubscriptionDO — atomic + idempotent (MONEY-SAFETY #5).
    let sub = sub_stub(env, user_id)?;
    do_post(
        &sub,
        "/activate",
        &serde_json::json!({
            "periodEnd": period_end,
            "provider": provider,
            "contractId": contract_id,
            "email": email,
            "activateKey": format!("claim:{claim_id}"),
        }),
    )
    .await?;

    // Map contract → userId so future renewals resolve to this user, and drop the
    // stale guest mapping so a renewal can never re-enter the guest path (#3).
    if let Some(cid) = &contract_id {
        index_put(env, &format!("contract:{cid}"), user_id).await?;
        index_delete(env, &format!("claim-contract:{cid}")).await?;
    }

    relay(do_get(&sub, "/subscription").await?).await
}

// ── POST /cancel (app-JWT) ────────────────────────────────────────────────────
async fn cancel(env: &Env, user_id: &str) -> Result<Response> {
    let sub = sub_stub(env, user_id)?;
    let mut cur_res = do_get(&sub, "/subscription").await?;
    let cur: serde_json::Value = cur_res.json().await?;
    let provider_name = cur.get("provider").and_then(|v| v.as_str()).unwrap_or("");
    let contract_id = cur.get("contractId").and_then(|v| v.as_str()).map(String::from);
    let email = cur
        .get("email")
        .and_then(|v| v.as_str())
        .map(String::from)
        .unwrap_or_else(|| format!("{user_id}@users.renorma.app"));

    let provider = if !provider_name.is_empty() {
        match provider_for_env(provider_name, env).await {
            Ok(p) => p,
            Err(reason) => {
                console_error!("cancel: {reason}");
                return Ok(error_response_detail("MISCONFIGURED", &reason, 503));
            }
        }
    } else {
        None
    };

    if let (Some(p), Some(cid)) = (&provider, &contract_id) {
        // If lava's DELETE fails the recurring contract stays ACTIVE and keeps
        // charging — we must NOT report success (lava has no refund). Fail loudly,
        // do NOT mark no-renew locally (that would lie). CLAUDE.md: never swallow.
        if let Err(e) = p.cancel(cid, &email).await {
            console_error!(
                "/cancel: provider.cancel failed for user={user_id} contract={cid}: {e}"
            );
            return Ok(error_response_detail("lava_cancel_failed", &e.to_string(), 502));
        }
    }

    let out = do_post(&sub, "/cancel", &serde_json::json!({})).await?;
    relay(out).await
}

/// Prorated refund for the user's ACTIVE subscription, per the agreed formula:
///   last-payment price − 8% commission → /30 = daily rate → × days-left-to-`end`,
///   rounded. Price is the (single) plan's config price (the sub record drops the
///   planId, and the stored claim amount == plan price, so this matches the charge).
///   Returns None when there's nothing to refund (no active sub).
struct RefundCalc {
    amount: i64,
    currency: String,
    days_left: i64,
    contract_id: Option<String>,
    email: Option<String>,
}

async fn compute_refund(env: &Env, user_id: &str) -> Result<Option<RefundCalc>> {
    let sub = sub_stub(env, user_id)?;
    let mut r = do_get(&sub, "/subscription").await?;
    let s: serde_json::Value = r.json().await?;
    if !s.get("active").and_then(|v| v.as_bool()).unwrap_or(false) {
        return Ok(None);
    }
    let end = s.get("end").and_then(|v| v.as_i64()).unwrap_or(0);
    let contract_id = s.get("contractId").and_then(|v| v.as_str()).map(String::from);

    // Price = what the buyer ACTUALLY paid last (promo applied), read from lava for
    // this contract. Fall back to the plan's list price only if lava can't be reached.
    let (price, currency) = last_paid_amount(env, contract_id.as_deref()).await;

    let now = Date::now().as_millis() as i64;
    let day = 86_400_000i64;
    let days_left = (((end - now) + day - 1) / day).max(0);
    let daily = price * 0.92 / 30.0;
    let amount = (daily * days_left as f64).round() as i64;
    Ok(Some(RefundCalc {
        amount,
        currency,
        days_left,
        contract_id,
        email: s.get("email").and_then(|v| v.as_str()).map(String::from),
    }))
}

/// The buyer's actual last-payment amount (promo applied) from lava for their contract.
/// Falls back to the plan's config list price if there's no contract or lava is down —
/// logged loudly, so an operator can spot a refund computed off the list price.
async fn last_paid_amount(env: &Env, contract_id: Option<&str>) -> (f64, String) {
    if let Some(cid) = contract_id {
        match provider_for_env("lava", env).await {
            Ok(Some(p)) if p.configured() => match p.last_payment(cid).await {
                Ok(Some((amt, cur))) => return (amt, cur),
                Ok(None) => console_warn!("refund: no completed lava invoice for contract {cid} — using plan price"),
                Err(e) => console_error!("refund: lava last_payment({cid}) failed: {e} — using plan price"),
            },
            Ok(_) => {}
            Err(reason) => console_error!("refund: lava provider unavailable: {reason} — using plan price"),
        }
    }
    let plan = plans(env).into_iter().next();
    let price = plan
        .as_ref()
        .and_then(|p| p.get("price"))
        .and_then(|v| v.as_f64())
        .unwrap_or(0.0);
    let currency = plan
        .as_ref()
        .and_then(|p| p.get("currency"))
        .and_then(|v| v.as_str())
        .unwrap_or("RUB")
        .to_string();
    (price, currency)
}

async fn refund_preview(env: &Env, user_id: &str) -> Result<Response> {
    match compute_refund(env, user_id).await? {
        Some(c) => Response::from_json(&serde_json::json!({
            "amount": c.amount, "currency": c.currency, "daysLeft": c.days_left,
        })),
        None => Ok(error_response("no_active_subscription", 400)),
    }
}

async fn refund_request(env: &Env, user_id: &str) -> Result<Response> {
    let calc = match compute_refund(env, user_id).await? {
        Some(c) => c,
        None => return Ok(error_response("no_active_subscription", 400)),
    };
    // 1) Record the request for the operator (lava has no refund API → manual in lava).
    let claim = claim_stub(env)?;
    do_post(
        &claim,
        "/refund-add",
        &serde_json::json!({
            "userId": user_id,
            "amount": calc.amount,
            "currency": calc.currency,
            "contractId": calc.contract_id,
            "email": calc.email,
            "daysLeft": calc.days_left,
        }),
    )
    .await?;
    // 2) Revoke access immediately.
    let sub = sub_stub(env, user_id)?;
    do_post(&sub, "/refund", &serde_json::json!({})).await?;
    Response::from_json(&serde_json::json!({
        "ok": true, "amount": calc.amount, "currency": calc.currency,
    }))
}

fn error_response_detail(message: &str, detail: &str, status: u16) -> Response {
    Response::from_json(&serde_json::json!({ "error": message, "detail": detail }))
        .expect("serialize error")
        .with_status(status)
}

// ── POST /webhook/:provider — exact resolution order (renewal fix) ─────────────
async fn webhook(mut req: Request, env: &Env, name: &str) -> Result<Response> {
    let provider = match provider_for_env(name, env).await {
        Ok(Some(p)) => p,
        Ok(None) => return Ok(error_response("unknown_provider", 404)),
        Err(reason) => {
            console_error!("webhook: {reason}");
            return Ok(error_response_detail("MISCONFIGURED", &reason, 503));
        }
    };

    let (ok, body) = provider.verify_webhook(&mut req).await;
    if !ok {
        return Ok(error_response("invalid_signature", 401));
    }
    let raw = body.unwrap_or(serde_json::json!({}));
    let ev = provider.parse_webhook(&raw);
    let ek = event_key(name, &ev, &raw);

    let mut contract_ids: Vec<String> = vec![];
    if let Some(c) = &ev.contract_id {
        contract_ids.push(c.clone());
    }
    if let Some(c) = &ev.parent_contract_id {
        contract_ids.push(c.clone());
    }

    // ── USER resolution FIRST (#3): is this contract already bound to a user? ──
    // MUST precede the guest path so renewals of a claimed sub reach the user's
    // SubscriptionDO /activate (renewal-misrouting fix).
    let mut user_id: Option<String> = None;
    if let Some(c) = &ev.contract_id {
        user_id = index_get(env, &format!("contract:{c}")).await?;
    }
    if user_id.is_none() {
        if let Some(p) = &ev.parent_contract_id {
            user_id = index_get(env, &format!("contract:{p}")).await?;
        }
    }

    // ── GUEST resolution: only when the contract is not already bound to a user. ──
    let mut guest_claim_id: Option<String> = None;
    let mut guest_contract: Option<String> = None;
    if user_id.is_none() {
        for cid in &contract_ids {
            if let Some(found) = index_get(env, &format!("claim-contract:{cid}")).await? {
                guest_claim_id = Some(found);
                guest_contract = Some(cid.clone()); // the cid that matched (NOT contract_ids[0])
                break;
            }
        }
    }
    if let (Some(gid), Some(gcontract)) = (&guest_claim_id, &guest_contract) {
        match ev.kind {
            WebhookKind::Paid | WebhookKind::Recurring => {
                let claim = claim_stub(env)?;
                let mut r = do_post(
                    &claim,
                    "/mark-paid",
                    &serde_json::json!({
                        "contractId": gcontract,
                        "periodEnd": ev.period_end,
                        "email": ev.email,
                        "eventKey": ek,
                        "amount": ev.amount,
                        "currency": ev.currency,
                    }),
                )
                .await?;
                let rj: serde_json::Value = r.json().await.unwrap_or(serde_json::json!({}));
                if rj.get("tombstoned").and_then(|v| v.as_bool()).unwrap_or(false) {
                    console_error!(
                        "webhook: paid event for VOID guest claim {gid} contract={gcontract} — ignored"
                    );
                } else if rj.get("mapped").and_then(|v| v.as_bool()) == Some(false) {
                    console_error!(
                        "webhook: claim-contract index pointed at {gid} but ClaimDO has no row for contract={gcontract}"
                    );
                } else if rj.get("paid").and_then(|v| v.as_bool()) == Some(true) {
                    // Genuine pending→paid transition: notify telegram-worker so the
                    // bot delivers the claim-binding link. Best-effort (never fails
                    // the webhook); telegram-worker is idempotent regardless.
                    // duplicate/alreadyPaid/claimed → no re-notify.
                    notify_telegram_paid(env, gid).await;
                }
                return Response::from_json(&serde_json::json!({ "ok": true, "guest": true }));
            }
            WebhookKind::Refunded => {
                let claim = claim_stub(env)?;
                let vr = do_post(
                    &claim,
                    "/void-by-contract",
                    &serde_json::json!({ "contractId": gcontract }),
                )
                .await?;
                console_warn!(
                    "webhook: refund for guest claim {gid} contract={gcontract} → void status={}",
                    vr.status_code()
                );
                return Response::from_json(
                    &serde_json::json!({ "ok": true, "guest": true, "voided": true }),
                );
            }
            _ => {
                console_warn!(
                    "webhook: {} for unclaimed guest claim {gid} — no-op",
                    kind_str(&ev.kind)
                );
                return Response::from_json(&serde_json::json!({ "ok": true, "guest": true }));
            }
        }
    }

    // ── USER path: renewals/cancels of an already-claimed (bound) subscription. ──
    // Fall back to the synthetic email passed at checkout (AFTER the guest return).
    if user_id.is_none() {
        if let Some(em) = &ev.email {
            if em.ends_with("@users.renorma.app") {
                user_id = em.split('@').next().map(String::from);
            }
        }
    }
    let user_id = match user_id {
        Some(u) => u,
        None => {
            console_warn!(
                "webhook: unmapped event kind={} eventKey={ek} — acked, no-op",
                kind_str(&ev.kind)
            );
            return Response::from_json(&serde_json::json!({ "ok": true, "mapped": false }));
        }
    };

    // Root (parent) contract id — what cancel() needs and what recurring events reference.
    let root_contract = ev
        .parent_contract_id
        .clone()
        .or_else(|| ev.contract_id.clone());
    if let Some(c) = &ev.contract_id {
        index_put(env, &format!("contract:{c}"), &user_id).await?;
    }
    if let Some(rc) = &root_contract {
        index_put(env, &format!("contract:{rc}"), &user_id).await?;
    }

    let sub = sub_stub(env, &user_id)?;
    match ev.kind {
        WebhookKind::Paid | WebhookKind::Recurring => {
            do_post(
                &sub,
                "/activate",
                &serde_json::json!({
                    "periodEnd": ev.period_end,
                    "provider": name,
                    "contractId": root_contract,
                    "email": ev.email,
                    "activateKey": ek,
                }),
            )
            .await?;
            let msg = if ev.kind == WebhookKind::Recurring {
                "Подписка продлена. Спасибо!"
            } else {
                "Оплата прошла успешно — подписка активна!"
            };
            notify_push(env, &user_id, msg, "/settings/subscription").await;
        }
        WebhookKind::Cancelled => {
            do_post(
                &sub,
                "/cancel",
                &serde_json::json!({ "periodEnd": ev.period_end }),
            )
            .await?;
        }
        WebhookKind::Refunded => {
            do_post(&sub, "/refund", &serde_json::json!({})).await?;
        }
        WebhookKind::Failed => {}
    }
    Response::from_json(&serde_json::json!({ "ok": true }))
}

// Keep PROVIDER_NAMES referenced (parity with TS export; not otherwise used).
#[allow(dead_code)]
fn _provider_names() -> &'static [&'static str] {
    providers::PROVIDER_NAMES
}

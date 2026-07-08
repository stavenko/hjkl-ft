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
mod usage_do;

pub use claim_do::ClaimDO;
pub use payment_index_do::PaymentIndexDO;
pub use subscription_do::SubscriptionDO;
pub use usage_do::UsageDO;

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
const DO_EPOCH: &str = "v5";

/// How long a created lava invoice is considered payable. The status endpoint reports a
/// pending invoice as expired past this window (the Mini App then shows a «create new
/// invoice» action). Adjust to lava's real invoice lifetime.
const INVOICE_TTL_MS: i64 = 60 * 60 * 1000; // 60 minutes

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
/// The single global neuro-token usage ledger. A fresh store (no epoch): it holds no
/// money-safety state, so there is nothing to wipe on a DO_EPOCH bump.
fn usage_stub(env: &Env) -> Result<worker::durable::Stub> {
    env.durable_object("USAGE_DO")?
        .id_from_name("usage")?
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

/// True when the provider talks to the REAL lava host (real money). The lava-mock is
/// NOT real money, so it may run on the test env alongside TEST_ENTITLEMENT.
fn real_money_provider(env: &Env) -> bool {
    env.var("LAVA_API_URL").map(|v| v.to_string()).unwrap_or_default() == "https://gate.lava.top"
}

/// The free-sub test path and REAL money are mutually exclusive: an env that mints free
/// test subs must NEVER also take real money. Blocks the real-checkout routes only when
/// BOTH hold — so the lava-mock (fake money) checkout stays reachable on the test env.
fn free_sub_blocks_checkout(env: &Env) -> bool {
    test_entitlement_on(env) && real_money_provider(env)
}


// ── claim-secret crypto (MONEY-SAFETY #1) ─────────────────────────────────────
/// 256-bit (>=128-bit) random secret, base64url, no padding. Used both for the
/// opaque public claimId AND the high-entropy claim secret.
fn random_claim_secret() -> Result<String> {
    let mut bytes = [0u8; 32];
    getrandom::getrandom(&mut bytes).map_err(|e| Error::RustError(format!("getrandom: {e}")))?;
    Ok(URL_SAFE_NO_PAD.encode(bytes))
}

/// Atomically fetch the next global bill sequence from ClaimDO (single-instance DO →
/// race-free). Each issued Telegram invoice consumes one, making its buyer/receipt email
/// unique + collision-proof.
async fn next_bill_seq(env: &Env) -> Result<i64> {
    let claim = claim_stub(env)?;
    let mut r = do_post(&claim, "/next-bill-seq", &serde_json::json!({})).await?;
    let v: serde_json::Value = r.json().await?;
    v.get("value")
        .and_then(|x| x.as_i64())
        .ok_or_else(|| Error::RustError("next-bill-seq: no value".into()))
}

/// Reduce a Telegram username to an email-local-part-safe token: lowercase, keep only
/// `[a-z0-9_]`. Telegram usernames already fit this set; this is defensive. The dot is
/// deliberately dropped — it separates the `tg.<ident>.<seq>` fields of the address.
fn email_ident(s: &str) -> String {
    s.chars()
        .map(|c| c.to_ascii_lowercase())
        .filter(|c| c.is_ascii_alphanumeric() || *c == '_')
        .collect()
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

/// Build a provider with the API base URL + credentials resolved per env. Credentials:
/// PROD reads them from the Secrets Store; DEV/test reads them from plain `[vars]` (the
/// lava-mock keys) — real lava never sees a dev value. A present-but-unresolvable LAVA
/// store binding (prod misconfig) propagates Err loudly.
///
/// MONEY-SAFETY: if REAL (Secrets-Store) creds resolve, the base URL MUST be
/// gate.lava.top — real creds can never be pointed at the mock (a mock+real-creds pair
/// would let a dev URL move real money). Fail loud otherwise.
async fn provider_for_env(name: &str, env: &Env) -> std::result::Result<Option<Lava>, String> {
    let base = env.var("LAVA_API_URL").map(|v| v.to_string()).unwrap_or_default();

    let store_api_key = read_secret_store(env, "LAVA_API_KEY").await?;
    let store_hook = read_secret_store(env, "LAVA_WEBHOOK_SECRET").await?;
    let creds_from_store = store_api_key.is_some();

    let var_nonempty = |k: &str| env.var(k).ok().map(|v| v.to_string()).filter(|s| !s.is_empty());
    let api_key = store_api_key.or_else(|| var_nonempty("LAVA_API_KEY"));
    let webhook_secret = store_hook.or_else(|| var_nonempty("LAVA_WEBHOOK_SECRET"));

    // Real Secrets-Store creds may ONLY talk to the real lava host.
    if creds_from_store && base != "https://gate.lava.top" {
        return Err(format!(
            "MISCONFIGURED: real lava creds with LAVA_API_URL='{base}' (must be gate.lava.top) — refusing (money-safety)"
        ));
    }
    // Configured but no base → loud misconfig (never default a base silently).
    if api_key.is_some() && base.is_empty() {
        return Err("MISCONFIGURED: LAVA_API_URL not set".into());
    }

    // DEV: reach the lava-mock via a service binding (same-zone worker→worker fetch is
    // blocked, error 1042). Absent in prod → real internet fetch to gate.lava.top.
    let mock = env.service("LAVA_MOCK").ok();

    Ok(provider_for(name, base, mock, api_key, webhook_secret))
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
    // Paid users who have NOT set up durable access (no passkey) — new-model worklist.
    if method == Method::Get && path == "/admin/paid-no-access" {
        if let Err(resp) = require_admin(&req, env).await {
            return Ok(resp);
        }
        return admin_paid_no_access(env).await;
    }
    // Client-requested refunds (access already revoked); operator processes each in lava.
    if method == Method::Get && path == "/admin/refunds" {
        if let Err(resp) = require_admin(&req, env).await {
            return Ok(resp);
        }
        let stub = claim_stub(env)?;
        return relay(do_get(&stub, "/refunds").await?).await;
    }
    // Admin: neuro-token usage aggregate (per-user totals+split, per-day totals, grand total).
    if method == Method::Get && path == "/admin/usage" {
        if let Err(resp) = require_admin(&req, env).await {
            return Ok(resp);
        }
        let stub = usage_stub(env)?;
        return relay(do_get(&stub, "/report").await?).await;
    }
    // Admin: recent caught receipts (each bound to its payment) — list view.
    if method == Method::Get && path == "/admin/receipts" {
        if let Err(resp) = require_admin(&req, env).await {
            return Ok(resp);
        }
        let stub = claim_stub(env)?;
        return relay(do_get(&stub, "/receipt/recent").await?).await;
    }
    // Admin: one receipt's FULL body by ?id= (detail view).
    if method == Method::Get && path == "/admin/receipt" {
        if let Err(resp) = require_admin(&req, env).await {
            return Ok(resp);
        }
        let id = req
            .url()?
            .query_pairs()
            .find(|(k, _)| k == "id")
            .map(|(_, v)| v.into_owned())
            .unwrap_or_default();
        if id.is_empty() {
            return Ok(error_response("missing id", 400));
        }
        let stub = claim_stub(env)?;
        return relay(do_post(&stub, "/receipt/get", &serde_json::json!({ "id": id })).await?).await;
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
    // The offer's LIST price for a currency (no invoice minted) — the Mini App "ценник"
    // before any promo. INTERNAL_PUSH_KEY-gated.
    if method == Method::Post && path == "/internal/price" {
        return internal_price(req, env).await;
    }
    // The subscription status of the account a claim is bound to — so the Mini App can
    // show the LIVE status (active / cancelled + days left). INTERNAL_PUSH_KEY-gated.
    if method == Method::Post && path == "/internal/claim-subscription" {
        return internal_claim_subscription(req, env).await;
    }
    // The user's newest non-terminal claim (pending invoice + its deadline), so the Mini
    // App can show «pay invoice until <deadline>» / «create new invoice». INTERNAL_PUSH_KEY.
    if method == Method::Post && path == "/internal/active-by-tg" {
        return internal_active_by_tg(req, env).await;
    }
    // receipt-worker → bind a caught receipt email (address, amount, full text) to its payment.
    if method == Method::Post && path == "/internal/receipt" {
        return internal_receipt(req, env).await;
    }
    // ai-worker / ocr-queue → record neuro-token usage (best-effort on the caller side).
    if method == Method::Post && path == "/internal/usage" {
        return internal_usage(req, env).await;
    }
    // Telegram-binding reads/writes for telegram-worker (secret lives here now).
    if method == Method::Post && path.starts_with("/internal/tg/") {
        return internal_tg(req, env, &path["/internal/tg/".len()..].to_string()).await;
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
    /// The ACTUAL amount to charge, decoded from lava's paymentParams.amount_total
    /// (promo-applied). `None` when the decode missed — the client shows '…' rather
    /// than a fabricated price; the invoice remains payable.
    amount: Option<f64>,
    /// Currency of `amount` (from amount_total.currency). `None` alongside `amount`.
    amount_currency: Option<String>,
}

/// Resolve (or create) the universal user_id for an external identity via the AUTH_WORKER
/// binding — provider-agnostic, idempotent (first touch may already have created it). Best-
/// effort: on any failure we log loudly and return None so a checkout still succeeds and
/// falls back to the legacy claim path; the caller must NOT hard-fail a payment on this.
async fn resolve_account(
    env: &Env,
    provider: &str,
    provider_uid: &str,
    username: Option<&str>,
) -> Option<String> {
    let key = token::secret_or_var(env, "INTERNAL_PUSH_KEY").await.ok()?;
    let mut body = serde_json::json!({ "provider": provider, "providerUid": provider_uid });
    if let Some(u) = username {
        body["username"] = serde_json::Value::String(u.to_string());
    }
    let headers = Headers::new();
    headers.set("Content-Type", "application/json").ok()?;
    headers.set("X-Internal-Key", &key).ok()?;
    let mut init = RequestInit::new();
    init.with_method(Method::Post)
        .with_headers(headers)
        .with_body(Some(JsValue::from_str(&body.to_string())));
    let request = Request::new_with_init("https://auth-worker/internal/account-resolve", &init).ok()?;
    let auth = env.service("AUTH_WORKER").ok()?;
    let mut res = match auth.fetch_request(request).await {
        Ok(r) => r,
        Err(e) => {
            console_error!("resolve_account: fetch failed: {e}");
            return None;
        }
    };
    if !(200..300).contains(&res.status_code()) {
        console_error!("resolve_account: auth {} ", res.status_code());
        return None;
    }
    let v: serde_json::Value = res.json().await.ok()?;
    v.get("userId").and_then(|x| x.as_str()).map(String::from)
}

/// Ask auth-worker whether an account has any passkey. None on failure (treated as «unknown»,
/// surfaced by the admin so it's not silently hidden).
async fn auth_has_credentials(env: &Env, user_id: &str) -> Option<bool> {
    let key = token::secret_or_var(env, "INTERNAL_PUSH_KEY").await.ok()?;
    let payload = serde_json::json!({ "userId": user_id }).to_string();
    let headers = Headers::new();
    headers.set("Content-Type", "application/json").ok()?;
    headers.set("X-Internal-Key", &key).ok()?;
    let mut init = RequestInit::new();
    init.with_method(Method::Post)
        .with_headers(headers)
        .with_body(Some(JsValue::from_str(&payload)));
    let request =
        Request::new_with_init("https://auth-worker/internal/has-credentials", &init).ok()?;
    let auth = env.service("AUTH_WORKER").ok()?;
    let mut res = auth.fetch_request(request).await.ok()?;
    if !(200..300).contains(&res.status_code()) {
        return None;
    }
    let v: serde_json::Value = res.json().await.ok()?;
    v.get("hasCredentials").and_then(|x| x.as_bool())
}

/// Admin worklist: paid users who haven't set up durable access (no passkey). The signal the
/// operator acts on — «paid, but can't get in yet» → nudge them.
async fn admin_paid_no_access(env: &Env) -> Result<Response> {
    let claim = claim_stub(env)?;
    let mut r = do_get(&claim, "/paid-with-user").await?;
    let v: serde_json::Value = r.json().await?;
    let empty = vec![];
    let claims = v.get("claims").and_then(|x| x.as_array()).unwrap_or(&empty);
    let mut out: Vec<serde_json::Value> = vec![];
    for c in claims {
        let uid = c.get("user_id").and_then(|x| x.as_str()).unwrap_or("");
        if uid.is_empty() {
            continue;
        }
        // Surface those WITHOUT credentials (and «unknown» on a lookup error — never hide).
        if auth_has_credentials(env, uid).await != Some(true) {
            out.push(c.clone());
        }
    }
    Response::from_json(&serde_json::json!({ "users": out }))
}

/// The shared body of checkout: provider resolution, plan lookup, lava checkout creation,
/// ClaimDO /create-pending + contract→user index. Returns Err(Response) for every error case
/// (with the SAME statuses as before) so both callers surface identical errors; the PROD-ONLY
/// guard stays in each caller.
async fn do_guest_checkout(
    body: &serde_json::Value,
    env: &Env,
) -> std::result::Result<GuestCheckout, Response> {
    // One provider, one offer. The buyer never chooses this — it's a fixed constant, not a
    // default filled in for a missing body field.
    let provider_name = "lava".to_string();
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
    // The lava offer to sell. There is NO plan catalog in config — lava owns the plans
    // and the pricing; we keep only this single provider pointer. Fail loud if unset.
    let offer_id = env.var("LAVA_OFFER_ID").map(|v| v.to_string()).unwrap_or_default();
    if offer_id.is_empty() {
        console_error!("do_guest_checkout: LAVA_OFFER_ID not configured");
        return Err(error_response("provider_not_configured", 400));
    }
    let plan_id_owned = offer_id.clone();
    // Optional promo code (trimmed; empty → None). An empty/absent promo means «no promo»
    // (full price) — the client is authoritative, nothing is carried over from a prior claim.
    let promo_code = body
        .get("promoCode")
        .and_then(|v| v.as_str())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
    // Buyer currency — REQUIRED. RUB → Russian acquirer (RU cards); USD/EUR → international
    // acquirer (foreign cards). The client always sends it explicitly; missing or not one of
    // RUB/USD/EUR → 400. No silent fallback to RUB.
    let currency = match body
        .get("currency")
        .and_then(|v| v.as_str())
        .map(|s| s.trim().to_uppercase())
        .filter(|s| matches!(s.as_str(), "RUB" | "USD" | "EUR"))
    {
        Some(c) => c,
        None => return Err(error_response("currency_required", 400)),
    };
    // Buyer payment method — REQUIRED. Validated against lava's ACTUAL PaymentMethodType
    // enum. The client always sends it explicitly (the Mini App sends CARD, the only reliable
    // channel); missing or invalid → 400. No silent default, no currency-based fallback.
    let payment_method = match body
        .get("paymentMethod")
        .and_then(|v| v.as_str())
        .map(|s| s.trim().to_uppercase())
        .filter(|s| {
            matches!(
                s.as_str(),
                "CARD" | "SBP" | "PAYPAL" | "IDEAL" | "CHECKOUT_PAGE" | "MBWAY" | "BIZUM"
                    | "STRIPE" | "SEPATRANSFER" | "PIX" | "BANCONTACT" | "APPLE_PAY"
            )
        }) {
        Some(m) => m,
        None => return Err(error_response("payment_method_required", 400)),
    };
    // Optional Telegram identity (present only for the Mini App flow) — recorded on the
    // claim so an operator can reconcile «who paid / did they bind an account».
    let tg_user_id = body.get("tgUserId").and_then(|v| v.as_i64());
    let tg_username = body
        .get("tgUsername")
        .and_then(|v| v.as_str())
        .map(|s| s.trim_start_matches('@').trim().to_string())
        .filter(|s| !s.is_empty());
    // Universal user_id for this identity (first touch may have created it; idempotent). We
    // bind the claim + contract to it below so the paid webhook activates the subscription
    // directly for user_id. Best-effort — None falls back to the legacy claim path.
    let user_id = match tg_user_id {
        Some(uid) => resolve_account(env, "telegram", &uid.to_string(), tg_username.as_deref()).await,
        None => None,
    };
    let claim = match claim_stub(env) {
        Ok(s) => s,
        Err(e) => return Err(error_response(&e.to_string(), 500)),
    };

    // Duplicate-purchase guard: if this Telegram user already has a PAID/CLAIMED entitlement,
    // refuse (ALREADY_ACTIVE 409) so the caller routes them into the app instead of minting a
    // second subscription. Money-safety, not idempotency: every checkout mints a fresh invoice
    // (the client caches the pay link per its own config), so there's no invoice to reuse here.
    // Only the Mini App flow carries tgUserId; a landing guest (no identity) has nothing to
    // dedup against and just mints.
    if let Some(uid) = tg_user_id {
        match do_post(&claim, "/active-by-tg", &serde_json::json!({ "tgUserId": uid })).await {
            Ok(mut resp) if resp.status_code() == 200 => {
                let v: serde_json::Value = resp.json().await.unwrap_or(serde_json::json!({}));
                let status = v.get("status").and_then(|s| s.as_str()).unwrap_or("none");
                if matches!(status, "paid" | "claimed") {
                    return Err(error_response_detail(
                        "ALREADY_ACTIVE",
                        "an active subscription/claim already exists for this Telegram user",
                        409,
                    ));
                }
            }
            Ok(_) => {} // non-200 probe → best-effort, mint anyway.
            Err(e) => {
                // A dead probe must not block a real purchase — log and mint.
                console_error!("do_guest_checkout: active-by-tg probe failed: {e}");
            }
        }
    }

    let claim_id = random_claim_secret().map_err(|e| error_response(&e.to_string(), 500))?; // opaque public id (≠ contractId; #1)
    let secret = random_claim_secret().map_err(|e| error_response(&e.to_string(), 500))?; // high-entropy claim secret (256-bit)
    let secret_hash = sha256_hex(&secret);
    // LANDING_RETURN_URL is set in both dev+prod [vars]; no hardcoded prod-host fallback.
    let base = env
        .var("LANDING_RETURN_URL")
        .map(|v| v.to_string())
        .unwrap_or_default();
    // FRAGMENT, not query (#1): not sent to the server, not logged.
    let _return_url = format!("{base}#claim={claim_id}.{secret}");

    // Keep the client-supplied promo/currency/method to persist on the claim (operator
    // reconciliation) — the create_checkout call below consumes the originals.
    let promo_for_claim = promo_code.clone();
    let currency_for_claim = currency.clone();
    let method_for_claim = payment_method.clone();
    // Buyer email sent to lava MUST be unique per invoice: lava refuses (400) to create a
    // second subscription invoice for an email that already has an active subscription to the
    // offer. It ALSO doubles as the receipt address we can catch (Email Routing → Worker) and
    // bind back to the payer. For the Telegram flow we encode a STABLE identity (the @username
    // when present — readable — else the numeric tg id) plus a GLOBAL monotonic bill sequence:
    // `tg.<ident>.<seq>@rcpt.renorma.app`. The seq guarantees per-invoice uniqueness AND is
    // collision-proof even if a @username is released and reclaimed by another account. A
    // landing guest (no Telegram identity) uses the opaque `<claimId>@rcpt.renorma.app`
    // (one receiving subdomain for every flow — no separate guest.* domain).
    // Never a buyer field — never taken from the request body.
    let email = match tg_user_id {
        Some(tid) => {
            let seq = next_bill_seq(env)
                .await
                .map_err(|e| error_response(&e.to_string(), 500))?;
            let ident = tg_username
                .as_deref()
                .map(email_ident)
                .filter(|s| !s.is_empty())
                .unwrap_or_else(|| tid.to_string());
            format!("tg.{ident}.{seq}@rcpt.renorma.app")
        }
        None => format!("{claim_id}@rcpt.renorma.app"),
    };
    // Keep a copy for the claim row (receipt→payment mapping); create_checkout consumes the original.
    let email_for_claim = email.clone();
    let checkout = match provider
        .create_checkout(&CheckoutOpts {
            offer_id,
            email,
            return_url: _return_url,
            promo_code,
            currency,
            payment_method: Some(payment_method),
        })
        .await
    {
        Ok(c) => c,
        Err(e) => return Err(error_response(&format!("checkout_failed: {e}"), 502)),
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
            // Synthetic buyer/receipt email — stored on the claim so an incoming receipt maps here.
            "email": email_for_claim,
            // The lava-decoded amount (paymentParams.amount_total, promo-applied), stored
            // in MINOR units (×100) to match the claims.amount INTEGER column + the webhook
            // amount; a float here would silently become NULL via opt_i64. Null when the
            // decode missed. lava still owns the authoritative amount on the receipt.
            "amount": checkout.amount.map(|a| (a * 100.0).round() as i64),
            // The universal account this payment belongs to (resolved at first touch). Stored
            // so the admin can list «paid but no credentials» users.
            "userId": user_id,
            "tgUserId": tg_user_id,
            "tgUsername": tg_username,
            // The lava pay link, stored for status/reconciliation.
            "payUrl": checkout.url,
            // The buyer's actual promo / currency / channel, stored for operator reconciliation.
            "promoCode": promo_for_claim,
            "currency": currency_for_claim,
            "paymentMethod": method_for_claim,
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
    // Map contract → claimId so the paid webhook finds the row (marks it paid + notifies the
    // bot via the guest path, which ALSO activates the sub for the claim's user_id).
    if let Err(e) = index_put(env, &format!("claim-contract:{}", checkout.order_id), &claim_id).await {
        return Err(error_response(&e.to_string(), 500));
    }

    // Telegram flow (Mini App / bot): bind the claim to the tg user + store its secret in
    // ClaimDO — the single source of truth. FAIL LOUD (not fire-and-forget): without this
    // row the paid-push webhook can't find the binding → the user pays but gets no success
    // message / onboarding. INSERT OR IGNORE makes it safe to retry the whole checkout.
    if let Some(tid) = tg_user_id {
        let put = do_post(
            &claim,
            "/tg/put",
            &serde_json::json!({ "claimId": claim_id, "tgId": tid, "secret": secret }),
        )
        .await;
        match put {
            Ok(r) if r.status_code() == 200 => {}
            Ok(r) => return Err(error_response(&format!("tg binding failed: {}", r.status_code()), 500)),
            Err(e) => return Err(error_response(&format!("tg binding failed: {e}"), 500)),
        }
    }

    Ok(GuestCheckout {
        pay_url: checkout.url,
        claim_id,
        secret,
        amount: checkout.amount,
        amount_currency: checkout.currency,
    })
}

// ── POST /checkout/guest (NO JWT; PROD-ONLY) ──────────────────────────────────
async fn checkout_guest(mut req: Request, env: &Env) -> Result<Response> {
    // A free-sub env must NOT also take REAL money — but the lava-mock (fake money) may
    // run on the test env, so only block when real lava is configured too.
    if free_sub_blocks_checkout(env) {
        return Ok(error_response("Not found", 404));
    }
    let body: serde_json::Value = req.json().await.unwrap_or(serde_json::json!({}));
    let gc = match do_guest_checkout(&body, env).await {
        Ok(gc) => gc,
        Err(resp) => return Ok(resp),
    };
    // claimId only — NEVER the secret (it travels back via the lava fragment).
    // amount/currency are the lava-decoded price (parity with the internal flow).
    Response::from_json(&serde_json::json!({
        "payUrl": gc.pay_url,
        "claimId": gc.claim_id,
        "amount": gc.amount,
        "currency": gc.amount_currency,
        // The invoice lifetime, so the client can watch for expiry without hardcoding it.
        "ttlMs": INVOICE_TTL_MS,
    }))
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

    // Same mutual exclusivity: a free-sub env may take MOCK money (lava-mock) but never
    // REAL money — block only when real lava is configured.
    if free_sub_blocks_checkout(env) {
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
        // Non-sensitive: the lava-decoded price so the Mini App can show it without a
        // second round-trip. null when the decode missed (client shows '…').
        "amount": gc.amount,
        "currency": gc.amount_currency,
        // The invoice lifetime, so the client can watch for expiry without hardcoding it.
        "ttlMs": INVOICE_TTL_MS,
    }))
}

// ── POST /internal/claim-subscription (INTERNAL_PUSH_KEY-guarded) ─────────────
/// Given a Mini App claimId, return the LIVE subscription of the account it's bound to,
/// so the Mini App can show "active / cancelled + N days". `{bound:false}` when the
/// claim was paid but not yet onboarded (no account/sub yet).
async fn internal_claim_subscription(mut req: Request, env: &Env) -> Result<Response> {
    let key = match token::secret_or_var(env, "INTERNAL_PUSH_KEY").await {
        Ok(k) => k,
        Err(e) => {
            console_error!("internal_claim_subscription: {e}");
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

    let body: serde_json::Value = req.json().await.unwrap_or(serde_json::json!({}));
    let claim_id = body.get("claimId").and_then(|v| v.as_str()).unwrap_or("");
    if claim_id.is_empty() {
        return Ok(error_response("missing claimId", 400));
    }

    let claim = claim_stub(env)?;
    let mut cb = do_post(&claim, "/claimed-by", &serde_json::json!({ "claimId": claim_id })).await?;
    let cbv: serde_json::Value = cb.json().await?;
    let claimed_by = match cbv.get("claimedBy").and_then(|v| v.as_str()) {
        Some(u) if !u.is_empty() => u.to_string(),
        _ => return Response::from_json(&serde_json::json!({ "bound": false })),
    };

    let sub = sub_stub(env, &claimed_by)?;
    let mut sr = do_get(&sub, "/subscription").await?;
    let s: serde_json::Value = sr.json().await?;
    let end = s.get("end").and_then(|v| v.as_i64()).unwrap_or(0);
    let now = Date::now().as_millis() as i64;
    let day = 86_400_000i64;
    let days_left = (((end - now) + day - 1) / day).max(0);
    Response::from_json(&serde_json::json!({
        "bound": true,
        "subStatus": s.get("status"),
        "active": s.get("active"),
        "noRenew": s.get("no_renew"),
        "daysLeft": days_left,
    }))
}

// ── POST /internal/usage (INTERNAL_PUSH_KEY-guarded) ──────────────────────────
/// Record neuro-token usage into the global UsageDO. Called by ai-worker (source
/// "text") and ocr-queue (source "vision") over their PAYMENT service binding. The
/// key gate FAILS CLOSED: an unset INTERNAL_PUSH_KEY → 500 (never an unauthenticated
/// write); a mismatch → 403. tokens<=0 or an empty userId is a 200 no-op.
async fn internal_usage(mut req: Request, env: &Env) -> Result<Response> {
    let key = match token::secret_or_var(env, "INTERNAL_PUSH_KEY").await {
        Ok(k) => k,
        Err(e) => {
            console_error!("internal_usage: {e}");
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

    let body: serde_json::Value = req.json().await.unwrap_or(serde_json::json!({}));
    let user_id = body.get("userId").and_then(|v| v.as_str()).unwrap_or("").trim();
    let tokens = body.get("tokens").and_then(|v| v.as_i64()).unwrap_or(0);
    let source = match body.get("source").and_then(|v| v.as_str()) {
        Some("vision") => "vision",
        _ => "text",
    };
    // No-op (still 200) on nothing to record — a well-formed but empty report.
    if user_id.is_empty() || tokens <= 0 {
        return Response::from_json(&serde_json::json!({ "ok": true }));
    }

    let stub = usage_stub(env)?;
    do_post(
        &stub,
        "/add",
        &serde_json::json!({ "userId": user_id, "tokens": tokens, "source": source }),
    )
    .await?;
    Response::from_json(&serde_json::json!({ "ok": true }))
}

// ── POST /internal/tg/{op} (INTERNAL_PUSH_KEY-guarded) ────────────────────────
/// telegram-worker's window into the tg_claims table (the Telegram binding + claim
/// secret now live in ClaimDO). `op` ∈ get | by-user | mark-notified — thin proxies to
/// the ClaimDO ops, forwarding the request body verbatim.
async fn internal_tg(mut req: Request, env: &Env, op: &str) -> Result<Response> {
    let key = match token::secret_or_var(env, "INTERNAL_PUSH_KEY").await {
        Ok(k) => k,
        Err(e) => {
            console_error!("internal_tg: {e}");
            return Ok(error_response("internal_not_configured", 500));
        }
    };
    let provided = req.headers().get("X-Internal-Key").ok().flatten().unwrap_or_default();
    if provided.is_empty() || provided != key {
        return Ok(error_response("bad internal key", 403));
    }
    let do_path = match op {
        "get" => "/tg/get",
        "by-user" => "/tg/by-user",
        "mark-notified" => "/tg/mark-notified",
        _ => return Ok(error_response("unknown tg op", 404)),
    };
    let body: serde_json::Value = req.json().await.unwrap_or(serde_json::json!({}));
    let claim = claim_stub(env)?;
    let mut r = do_post(&claim, do_path, &body).await?;
    let v: serde_json::Value = r.json().await?;
    Response::from_json(&v)
}

// ── POST /internal/price (INTERNAL_PUSH_KEY-guarded) ──────────────────────────
/// The LAVA_OFFER_ID list price for a currency (RUB/USD/EUR), read from lava's products
/// WITHOUT minting an invoice — the Mini App "ценник" before any promo. Returns
/// {amount, currency}; amount is null (client shows "…") when the price can't be read
/// (never a fabricated number). A hard provider/HTTP failure → 502.
async fn internal_price(mut req: Request, env: &Env) -> Result<Response> {
    let key = match token::secret_or_var(env, "INTERNAL_PUSH_KEY").await {
        Ok(k) => k,
        Err(e) => {
            console_error!("internal_price: {e}");
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

    let body: serde_json::Value = req.json().await.unwrap_or(serde_json::json!({}));
    // Currency REQUIRED — the client always sends it. Missing/invalid → 400, no RUB fallback.
    let currency = match body
        .get("currency")
        .and_then(|v| v.as_str())
        .map(|s| s.trim().to_uppercase())
        .filter(|s| matches!(s.as_str(), "RUB" | "USD" | "EUR"))
    {
        Some(c) => c,
        None => return Ok(error_response("currency_required", 400)),
    };

    let offer_id = env.var("LAVA_OFFER_ID").map(|v| v.to_string()).unwrap_or_default();
    if offer_id.is_empty() {
        console_error!("internal_price: LAVA_OFFER_ID not configured");
        return Ok(error_response("provider_not_configured", 400));
    }
    let provider = match provider_for_env("lava", env).await {
        Ok(Some(p)) if p.configured() => p,
        Ok(_) => return Ok(error_response("provider_not_configured", 400)),
        Err(reason) => {
            console_error!("internal_price: {reason}");
            return Ok(error_response("provider_not_configured", 503));
        }
    };
    match provider.offer_price(&offer_id, &currency).await {
        Ok(Some((amount, cur))) => {
            Response::from_json(&serde_json::json!({ "amount": amount, "currency": cur }))
        }
        Ok(None) => Response::from_json(&serde_json::json!({ "amount": null, "currency": currency })),
        Err(e) => {
            console_error!("internal_price: offer_price failed: {e}");
            Ok(error_response("price_unavailable", 502))
        }
    }
}

// ── POST /internal/active-by-tg (INTERNAL_PUSH_KEY-guarded) ───────────────────
/// The Telegram user's newest non-terminal claim. When it's a `pending` invoice, return
/// its payUrl + deadline (created_at + INVOICE_TTL) + whether it's already expired, so the
/// Mini App shows «pay invoice until <deadline>» while valid and «create new invoice» once
/// expired. Non-pending (paid/claimed/none) → `{pending:false}`.
async fn internal_active_by_tg(mut req: Request, env: &Env) -> Result<Response> {
    let key = match token::secret_or_var(env, "INTERNAL_PUSH_KEY").await {
        Ok(k) => k,
        Err(e) => {
            console_error!("internal_active_by_tg: {e}");
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

    let body: serde_json::Value = req.json().await.unwrap_or(serde_json::json!({}));
    let tg_user_id = match body.get("tgUserId").and_then(|v| v.as_i64()) {
        Some(id) => id,
        None => return Ok(error_response("missing tgUserId", 400)),
    };

    let claim = claim_stub(env)?;
    let mut r = do_post(&claim, "/active-by-tg", &serde_json::json!({ "tgUserId": tg_user_id })).await?;
    let v: serde_json::Value = r.json().await?;
    let status = v.get("status").and_then(|s| s.as_str()).unwrap_or("none");
    if status != "pending" {
        return Response::from_json(&serde_json::json!({ "pending": false, "status": status }));
    }
    let created_at = v.get("createdAt").and_then(|s| s.as_i64()).unwrap_or(0);
    let deadline = created_at + INVOICE_TTL_MS;
    let expired = Date::now().as_millis() as i64 >= deadline;

    // Ask lava directly whether this invoice was actually paid — don't rely on the
    // webhook alone. A COMPLETED lava payment for the claim's contract → lavaPaid=true.
    // (STATUS ONLY: we do NOT mark the claim paid here.)
    let mut lava_paid = false;
    if let Some(cid) = v.get("contractId").and_then(|s| s.as_str()) {
        if let Ok(Some(provider)) = provider_for_env("lava", env).await {
            if provider.configured() {
                match provider.last_payment(cid).await {
                    Ok(Some(_)) => lava_paid = true,
                    Ok(None) => {}
                    Err(e) => console_error!("active-by-tg: lava last_payment({cid}) failed: {e}"),
                }
            }
        }
    }

    Response::from_json(&serde_json::json!({
        "pending": true,
        "claimId": v.get("claimId"),
        "payUrl": v.get("payUrl"),
        "deadline": deadline,
        "expired": expired,
        "lavaPaid": lava_paid,
    }))
}

// ── POST /internal/receipt (INTERNAL_PUSH_KEY-guarded) ────────────────────────
/// receipt-worker calls this after archiving a caught receipt email to R2. Resolves the
/// recipient address → its payment (claim, case-insensitively — inbound addresses arrive
/// lowercased) and stores the receipt (full text + amount) bound to it. Idempotent on the
/// email Message-ID (ClaimDO INSERT OR IGNORE). Unknown address → {bound:false} (the raw
/// stays archived in R2 regardless). The caller has ALREADY verified the sender is lava.
async fn internal_receipt(mut req: Request, env: &Env) -> Result<Response> {
    let key = match token::secret_or_var(env, "INTERNAL_PUSH_KEY").await {
        Ok(k) => k,
        Err(e) => {
            console_error!("internal_receipt: {e}");
            return Ok(error_response("internal_not_configured", 500));
        }
    };
    let provided = req.headers().get("X-Internal-Key").ok().flatten().unwrap_or_default();
    if provided.is_empty() || provided != key {
        return Ok(error_response("bad internal key", 403));
    }

    let body: serde_json::Value = req.json().await.unwrap_or(serde_json::json!({}));
    let email = body
        .get("email")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .trim()
        .to_string();
    if email.is_empty() {
        return Ok(error_response("missing email", 400));
    }

    let claim = claim_stub(env)?;
    let mut r = do_post(&claim, "/claim-by-email", &serde_json::json!({ "email": email })).await?;
    let cv: serde_json::Value = r.json().await?;
    let claim_id = if cv.get("found").and_then(|b| b.as_bool()) == Some(true) {
        cv.get("claimId").and_then(|v| v.as_str()).unwrap_or("").to_string()
    } else {
        console_warn!("internal_receipt: no claim for address {email} — archived only");
        return Response::from_json(&serde_json::json!({ "ok": true, "bound": false }));
    };

    let receipt_id = random_claim_secret()?;
    let add = do_post(
        &claim,
        "/receipt/add",
        &serde_json::json!({
            "id": receipt_id,
            "claimId": claim_id,
            "messageId": body.get("messageId"),
            "amount": body.get("amount"),      // minor units (×100), integer
            "currency": body.get("currency"),
            "bodyText": body.get("bodyText"),  // full decoded receipt text/HTML
            "pdfKey": body.get("pdfKey"),      // R2 key when a PDF attachment was present
        }),
    )
    .await?;
    if add.status_code() != 200 {
        return Ok(error_response("receipt add failed", 500));
    }
    Response::from_json(&serde_json::json!({ "ok": true, "bound": true, "claimId": claim_id }))
}

// ── POST /test/guest-checkout (PRODUCTION-IMPOSSIBLE) ─────────────────────────
async fn test_guest_checkout(mut req: Request, env: &Env) -> Result<Response> {
    if !test_entitlement_on(env) {
        return Ok(error_response("Not found", 404));
    }
    let body: serde_json::Value = req.json().await.unwrap_or(serde_json::json!({}));
    // No plan catalog anymore — the test path just tags the claim with whatever planId
    // the test passes (or "test"); it never touches lava.
    let plan_id_owned = body
        .get("planId")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .unwrap_or("test")
        .to_string();

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

    let mut out = do_post(&sub, "/cancel", &serde_json::json!({})).await?;
    let sub_json: serde_json::Value = out.json().await?;
    // Echo the cancellation to the Telegram bot (best-effort; no-op if not linked).
    let end = sub_json.get("end").and_then(|v| v.as_i64()).unwrap_or(0);
    notify_bot_cancelled(env, user_id, end).await;
    Response::from_json(&sub_json)
}

/// Best-effort: tell the Telegram bot the user cancelled, so it can echo "cancelled —
/// access for N more days". Resolves the tg user via a claimed Mini App claim; silently
/// no-ops if the account isn't linked to Telegram.
async fn notify_bot_cancelled(env: &Env, user_id: &str, end_ms: i64) {
    let claim = match claim_stub(env) {
        Ok(c) => c,
        Err(_) => return,
    };
    let mut r = match do_post(&claim, "/tg-for-user", &serde_json::json!({ "userId": user_id })).await {
        Ok(r) => r,
        Err(e) => {
            console_error!("notify_bot_cancelled: tg-for-user: {e}");
            return;
        }
    };
    let v: serde_json::Value = match r.json().await {
        Ok(v) => v,
        Err(_) => return,
    };
    let tg_user_id = match v.get("tgUserId").and_then(|x| x.as_i64()) {
        Some(id) => id,
        None => return, // account not linked to a Telegram user
    };
    let now = Date::now().as_millis() as i64;
    let day = 86_400_000i64;
    let days_left = (((end_ms - now) + day - 1) / day).max(0);

    let key = match token::secret_or_var(env, "INTERNAL_PUSH_KEY").await {
        Ok(k) if !k.is_empty() => k,
        _ => {
            console_warn!("notify_bot_cancelled: INTERNAL_PUSH_KEY not configured — skipping");
            return;
        }
    };
    let payload = serde_json::json!({ "tgUserId": tg_user_id, "daysLeft": days_left }).to_string();
    let headers = Headers::new();
    let _ = headers.set("Content-Type", "application/json");
    let _ = headers.set("X-Internal-Key", &key);
    let mut init = RequestInit::new();
    init.with_method(Method::Post)
        .with_headers(headers)
        .with_body(Some(JsValue::from_str(&payload)));
    let request = match Request::new_with_init("https://telegram-worker/internal/cancelled", &init) {
        Ok(r) => r,
        Err(e) => {
            console_error!("notify_bot_cancelled build request failed: {e}");
            return;
        }
    };
    let tg = match env.service("TELEGRAM_WORKER") {
        Ok(s) => s,
        Err(e) => {
            console_error!("notify_bot_cancelled: TELEGRAM_WORKER binding: {e}");
            return;
        }
    };
    match tg.fetch_request(request).await {
        Ok(mut res) => {
            let sc = res.status_code();
            if !(200..300).contains(&sc) {
                let t = res.text().await.unwrap_or_default();
                console_error!("notify_bot_cancelled: {sc} {t}");
            }
        }
        Err(e) => console_error!("notify_bot_cancelled failed: {e}"),
    }
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
    let email = s.get("email").and_then(|v| v.as_str()).map(String::from);
    let contract_id = match s.get("contractId").and_then(|v| v.as_str()) {
        Some(c) if !c.is_empty() => c.to_string(),
        _ => {
            console_error!("refund: subscription for {user_id} has no contract id");
            return Err(Error::RustError("refund_no_contract".into()));
        }
    };

    // Price = what the buyer ACTUALLY paid last (promo applied), from lava ONLY. There
    // is NO fallback: refunding real money off a config/list price would be wrong, so if
    // lava can't tell us the amount we fail loudly (→ 500) rather than guess.
    let provider = match provider_for_env("lava", env).await {
        Ok(Some(p)) if p.configured() => p,
        Ok(_) => {
            console_error!("refund: lava provider not configured");
            return Err(Error::RustError("refund_provider_unavailable".into()));
        }
        Err(reason) => {
            console_error!("refund: lava provider error: {reason}");
            return Err(Error::RustError("refund_provider_unavailable".into()));
        }
    };
    let (price, currency) = match provider.last_payment(&contract_id).await {
        Ok(Some(pc)) => pc,
        Ok(None) => {
            console_error!("refund: no completed lava payment for contract {contract_id}");
            return Err(Error::RustError("refund_no_payment".into()));
        }
        Err(e) => {
            console_error!("refund: lava last_payment({contract_id}) failed: {e}");
            return Err(Error::RustError("refund_lava_error".into()));
        }
    };

    let now = Date::now().as_millis() as i64;
    let day = 86_400_000i64;
    let days_left = (((end - now) + day - 1) / day).max(0);
    let daily = price * 0.92 / 30.0;
    let amount = (daily * days_left as f64).round() as i64;
    Ok(Some(RefundCalc {
        amount,
        currency,
        days_left,
        contract_id: Some(contract_id),
        email,
    }))
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
                    // Genuine pending→paid transition.
                    // NEW MODEL: the claim already carries the universal user_id (bound at
                    // checkout) → activate the sub for it NOW (subscription starts at payment),
                    // and map contract → user_id so future recurring events route to the user.
                    if let Some(uid) = rj.get("userId").and_then(|v| v.as_str()) {
                        let sub = sub_stub(env, uid)?;
                        do_post(
                            &sub,
                            "/activate",
                            &serde_json::json!({
                                "periodEnd": ev.period_end,
                                "provider": name,
                                "contractId": ev.parent_contract_id.clone().or_else(|| ev.contract_id.clone()),
                                "email": ev.email,
                                "activateKey": ek,
                            }),
                        )
                        .await?;
                        if let Some(c) = &ev.contract_id {
                            index_put(env, &format!("contract:{c}"), uid).await?;
                        }
                        if let Some(p) = &ev.parent_contract_id {
                            index_put(env, &format!("contract:{p}"), uid).await?;
                        }
                    }
                    // Notify telegram-worker so the bot delivers the access link. Best-effort
                    // (never fails the webhook); telegram-worker is idempotent regardless.
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

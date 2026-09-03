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
        // Приложение тренировок на своём dev-домене: подписку оно проверяет тем
        // же `GET /subscription`, что и приложение худеющего. В проде
        // gym.renorma.app проходит общим правилом суффикса — расходится только dev.
        || origin == "https://renorma-gym-dev.pages.dev"
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
        WebhookKind::Unknown => "unknown",
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

/// Admin: lava.top contracts (subscriptions) that are NOT bound to any account in our DB.
/// Groups lava invoices by their ROOT contract id (parentInvoice.id, else id), keeps the
/// latest by datetime per root, and returns only roots that are absent from BOTH the
/// `contract:<root>` (bound account) and `claim-contract:<root>` (pending claim) indexes.
async fn admin_lava_subscriptions(env: &Env) -> Result<Response> {
    let provider = match provider_for_env("lava", env).await {
        Ok(Some(p)) if p.configured() => p,
        _ => {
            return Response::from_json(&serde_json::json!({
                "subscriptions": [],
                "note": "lava provider not configured",
            }));
        }
    };

    // Latest invoice item per root contract id (dedupe, keep newest by `datetime`).
    let mut latest: std::collections::HashMap<String, serde_json::Value> = std::collections::HashMap::new();
    let mut collected = 0i64;
    for page in 1u32..=10 {
        let page_json = provider.list_invoices(page, 100).await?;
        let total = page_json.get("total").and_then(|v| v.as_i64()).unwrap_or(0);
        let items = match page_json.get("items").and_then(|v| v.as_array()) {
            Some(a) => a.clone(),
            None => break,
        };
        let n = items.len();
        for it in &items {
            let id = it.get("id").and_then(|v| v.as_str()).unwrap_or("");
            let root = it
                .get("parentInvoice")
                .and_then(|p| p.get("id"))
                .and_then(|v| v.as_str())
                .filter(|s| !s.is_empty())
                .unwrap_or(id)
                .to_string();
            if root.is_empty() {
                continue;
            }
            let dt = it.get("datetime").and_then(|v| v.as_str()).unwrap_or("");
            let keep = match latest.get(&root) {
                Some(prev) => dt >= prev.get("datetime").and_then(|v| v.as_str()).unwrap_or(""),
                None => true,
            };
            if keep {
                latest.insert(root, it.clone());
            }
        }
        collected += n as i64;
        if collected >= total || n < 100 {
            break;
        }
    }

    let mut out: Vec<serde_json::Value> = Vec::new();
    for (root, it) in latest {
        // Unbound iff neither the bound-account index nor the pending-claim index knows it.
        let bound = index_get(env, &format!("contract:{root}")).await?;
        let claim = index_get(env, &format!("claim-contract:{root}")).await?;
        if bound.is_some() || claim.is_some() {
            continue;
        }
        // The operator only needs contracts that can still charge money (to cancel
        // them) — i.e. LIVE recurring subscriptions. Pre-filter on the invoice-list
        // snapshot (subscriptionStatus is set only on subscription contracts), then
        // VERIFY against GET /subscriptions/{id} — the snapshot can keep saying
        // ACTIVE after the subscription actually died (observed live: an ACTIVE row
        // whose cancel returns «already cancelled or not a subscription»).
        if it.get("subscriptionStatus").and_then(|v| v.as_str()) != Some("ACTIVE") {
            continue;
        }
        if is_terminated(&it) {
            continue;
        }
        // Authoritative check. 404 → not a subscription; non-ACTIVE status or a set
        // cancelledAt/terminatedAt → dead, nothing to act on. A transport/API error
        // propagates loudly (never silently mis-list money state).
        let fresh = match provider.get_subscription(&root).await? {
            Some(f) => f,
            None => continue,
        };
        if fresh.get("subscriptionStatus").and_then(|v| v.as_str()) != Some("ACTIVE") {
            continue;
        }
        let dead = ["cancelledAt", "terminatedAt"].iter().any(|k| {
            fresh
                .get(*k)
                .and_then(|v| v.as_str())
                .map(|s| !s.is_empty())
                .unwrap_or(false)
        });
        if dead {
            continue;
        }
        // Prefer the fresh subscription fields (its buyer.email is what lava's
        // cancel endpoint matches against); fall back to the invoice snapshot.
        let amount = fresh
            .get("receipt")
            .and_then(|r| r.get("amount"))
            .filter(|v| !v.is_null())
            .cloned()
            .or_else(|| it.get("receipt").and_then(|r| r.get("amount")).cloned());
        let currency = fresh
            .get("receipt")
            .and_then(|r| r.get("currency"))
            .and_then(|v| v.as_str())
            .or_else(|| {
                it.get("receipt")
                    .and_then(|r| r.get("currency"))
                    .and_then(|v| v.as_str())
            });
        let email = fresh
            .get("buyer")
            .and_then(|b| b.get("email"))
            .and_then(|v| v.as_str())
            .or_else(|| {
                it.get("buyer")
                    .and_then(|b| b.get("email"))
                    .and_then(|v| v.as_str())
            });
        // expiredAt = when the paid period ends; for a live recurring contract that
        // is the moment lava attempts the next charge.
        let next_charge_at = fresh
            .get("expiredAt")
            .filter(|v| !v.is_null())
            .cloned()
            .or_else(|| {
                it.get("subscriptionDetails")
                    .and_then(|d| d.get("expiredAt"))
                    .cloned()
            });
        out.push(serde_json::json!({
            "contractId": root,
            "status": "ACTIVE",
            "amount": amount,
            "currency": currency,
            "datetime": it.get("datetime"),
            "email": email,
            "nextChargeAt": next_charge_at,
        }));
    }

    Response::from_json(&serde_json::json!({ "subscriptions": out }))
}

/// Admin: cancel a lava subscription. MONEY-SAFETY: lava has NO refund — this only stops
/// renewal. If the lava provider call FAILS we return an error and do NOT flip any local
/// state (mirrors the app-JWT /cancel). If the contract is bound to an account we also mark
/// the local SubscriptionDO no-renew (access kept until period end) and notify the bot.
async fn admin_cancel_subscription(mut req: Request, env: &Env) -> Result<Response> {
    let body: serde_json::Value = req.json().await.unwrap_or(serde_json::json!({}));
    let contract_id = body
        .get("contractId")
        .or_else(|| body.get("contract_id"))
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .trim()
        .to_string();
    if contract_id.is_empty() {
        return Ok(error_response("missing_params", 400));
    }

    // Resolve the buyer email: explicit body email wins; else derive from the bound account.
    let mut email = body
        .get("email")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .trim()
        .to_string();
    let bound_user = index_get(env, &format!("contract:{contract_id}")).await?;
    if email.is_empty() {
        if let Some(user) = &bound_user {
            email = format!("{user}@users.renorma.app");
        }
    }

    // Provider call FIRST. On misconfig → 503; on lava error → 502 and NO local flip.
    match provider_for_env("lava", env).await {
        Err(reason) => {
            console_error!("admin_cancel_subscription: {reason}");
            return Ok(error_response_detail("MISCONFIGURED", &reason, 503));
        }
        Ok(Some(p)) if p.configured() => {
            if let Err(e) = p.cancel(&contract_id, &email).await {
                console_error!(
                    "admin_cancel_subscription: provider.cancel failed for contract={contract_id}: {e}"
                );
                return Ok(error_response_detail("lava_cancel_failed", &e.to_string(), 502));
            }
        }
        Ok(_) => {}
    }

    // Bound account → mark local no-renew (keeps access until period end) + notify bot.
    if let Some(user_id) = bound_user {
        let sub = sub_stub(env, &user_id)?;
        let mut out = do_post(&sub, "/cancel", &serde_json::json!({})).await?;
        let sub_json: serde_json::Value = out.json().await?;
        let end = sub_json.get("end").and_then(|v| v.as_i64()).unwrap_or(0);
        notify_bot_cancelled(env, &user_id, end).await;
    }

    Response::from_json(&serde_json::json!({ "ok": true }))
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

    // Unauthenticated liveness probe (see the frontend `net` service). Wildcard
    // CORS + before secrets so it's a cheap, always-answerable 200 from any origin
    // (incl. per-deploy Pages hash subdomains).
    if req.method() == Method::Get && req.url().map(|u| u.path() == "/health").unwrap_or(false) {
        let headers = Headers::new();
        let _ = headers.set("Access-Control-Allow-Origin", "*");
        let _ = headers.set("Cache-Control", "no-store");
        return Ok(Response::ok("ok")?.with_headers(headers));
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

    // ── Public account state (NO JWT) ──
    // Установленный PWA знает свой user_id, но войти по ключу может не получиться.
    // Чтобы решить, предлагать ли вход по коду или отправлять человека платить,
    // нужно знать состояние аккаунта ДО входа. Отдаём два булевых значения и
    // ничего больше: ни дат, ни почты, ни провайдера, ни суммы.
    if method == Method::Post && path == "/account/state" {
        return account_state(req, env).await;
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
    // Admin: lava.top subscriptions/contracts NOT bound to any account in our DB.
    if method == Method::Get && path == "/admin/lava-subscriptions" {
        if let Err(resp) = require_admin(&req, env).await {
            return Ok(resp);
        }
        return admin_lava_subscriptions(env).await;
    }
    // Admin: cancel a lava subscription (stops renewal only — NO refund).
    // ── Account teardown for test accounts: who is this, and erase them ──
    if method == Method::Get && path == "/admin/users" {
        if let Err(resp) = require_admin(&req, env).await {
            return Ok(resp);
        }
        return admin_users(env).await;
    }

    if method == Method::Get && path == "/admin/user-card" {
        if let Err(resp) = require_admin(&req, env).await {
            return Ok(resp);
        }
        let user_id = url
            .query_pairs()
            .find(|(k, _)| k == "user_id")
            .map(|(_, v)| v.to_string())
            .unwrap_or_default();
        return admin_user_card(env, &user_id).await;
    }

    if method == Method::Post && path == "/admin/user-reset" {
        if let Err(resp) = require_admin(&req, env).await {
            return Ok(resp);
        }
        let mut body_req = req.clone()?;
        let body: serde_json::Value = body_req.json().await.unwrap_or(serde_json::json!({}));
        return admin_user_reset(env, &body).await;
    }

    if method == Method::Post && path == "/admin/user-wipe" {
        if let Err(resp) = require_admin(&req, env).await {
            return Ok(resp);
        }
        // Read the body from a clone: `handle` takes `req` immutably.
        let mut body_req = req.clone()?;
        let body: serde_json::Value = body_req.json().await.unwrap_or(serde_json::json!({}));
        return admin_user_wipe(env, &body).await;
    }

    // Админ: последние вебхуки провайдера С ТЕЛОМ — то, чего не хватило при разборе
    // сентябрьских отмен.
    if method == Method::Get && path == "/admin/webhook-events" {
        if let Err(resp) = require_admin(&req, env).await {
            return Ok(resp);
        }
        let stub = claim_stub(env)?;
        return relay(do_get(&stub, "/webhook-events").await?).await;
    }
    // Админ: разовая рассылка вдогонку тем, у кого подписка уже закончилась, а мы им
    // об этом так и не написали. По умолчанию — СУХОЙ ПРОГОН (список без отправки).
    if method == Method::Post && path == "/admin/notify-cancelled" {
        if let Err(resp) = require_admin(&req, env).await {
            return Ok(resp);
        }
        let mut body_req = req.clone()?;
        let body: serde_json::Value = body_req.json().await.unwrap_or(serde_json::json!({}));
        return admin_notify_cancelled(env, &body).await;
    }

    if method == Method::Post && path == "/admin/cancel-subscription" {
        if let Err(resp) = require_admin(&req, env).await {
            return Ok(resp);
        }
        return admin_cancel_subscription(req, env).await;
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
/// POST /account/state {userId} (ПУБЛИЧНО) → {active, entered}
///
/// Единственный вопрос, на который отвечает ручка: есть ли у этого аккаунта
/// доступ и доходил ли человек до приложения. Больше о нём ничего не сообщается —
/// user_id хоть и не секрет, но и подписка чужой человек по нему узнавать не должен.
///
/// `active` — тот же признак, по которому пускают ai-worker и ocr-queue
/// (см. GATE CONTRACT в subscription_do.rs). Ошибка похода за `entered` НЕ глушится
/// в `false`: это отдельное состояние, и врать про него нельзя — отвечаем 502.
async fn account_state(mut req: Request, env: &Env) -> Result<Response> {
    let body: serde_json::Value = req.json().await.unwrap_or(serde_json::json!({}));
    let user_id = body
        .get("userId")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .trim()
        .to_string();
    if user_id.is_empty() {
        return Ok(error_response("missing userId", 400));
    }

    let stub = sub_stub(env, &user_id)?;
    let mut r = do_get(&stub, "/subscription").await?;
    let sub: serde_json::Value = r.json().await?;
    let active = sub.get("active").and_then(|v| v.as_bool()).unwrap_or(false);

    let entered = match auth_has_entered(env, &user_id).await {
        Some(v) => v,
        None => {
            console_error!("account_state: auth-worker не ответил про entered для {user_id}");
            return Ok(error_response("auth_unavailable", 502));
        }
    };

    Response::from_json(&serde_json::json!({ "active": active, "entered": entered }))
}

/// Доходил ли пользователь до работающего приложения (открылась первая глава).
/// `None` — auth-worker не ответил; отличать это от «не доходил» обязательно.
async fn auth_has_entered(env: &Env, user_id: &str) -> Option<bool> {
    let v = auth_internal(env, "has-entered", user_id).await?;
    v.get("entered").and_then(|x| x.as_bool())
}

async fn auth_has_credentials(env: &Env, user_id: &str) -> Option<bool> {
    let v = auth_internal(env, "has-credentials", user_id).await?;
    v.get("hasCredentials").and_then(|x| x.as_bool())
}

/// Спросить auth-worker про user_id по внутренней ручке `/internal/<endpoint>`.
/// `None` — не дозвонились или он ответил отказом; вызывающий обязан отличать это
/// от отрицательного ответа.
async fn auth_internal(env: &Env, endpoint: &str, user_id: &str) -> Option<serde_json::Value> {
    let key = token::secret_or_var(env, "INTERNAL_PUSH_KEY").await.ok()?;
    let payload = serde_json::json!({ "userId": user_id }).to_string();
    let headers = Headers::new();
    headers.set("Content-Type", "application/json").ok()?;
    headers.set("X-Internal-Key", &key).ok()?;
    let mut init = RequestInit::new();
    init.with_method(Method::Post)
        .with_headers(headers)
        .with_body(Some(JsValue::from_str(&payload)));
    let url = format!("https://auth-worker/internal/{endpoint}");
    let request = Request::new_with_init(&url, &init).ok()?;
    let auth = env.service("AUTH_WORKER").ok()?;
    let mut res = auth.fetch_request(request).await.ok()?;
    if !(200..300).contains(&res.status_code()) {
        console_error!("auth_internal {endpoint}: HTTP {}", res.status_code());
        return None;
    }
    res.json().await.ok()
}

// ── Account teardown (test accounts): card + erase ───────────────────────────
// The destructive endpoint is admin-authenticated HERE (require_admin → the
// support worker's approved-admins table) and fans out over SERVICE BINDINGS.
// The wipe endpoints in the other workers answer 404 to anything that did not
// arrive over a binding, so this worker is the only door.

/// Every worker that owns per-user data: (binding, dummy host, label for the report).
const WIPE_TARGETS: &[(&str, &str, &str)] = &[
    ("AUTH_WORKER", "auth-worker", "аккаунт, ключи и токены"),
    ("SYNC_WORKER", "sync-worker", "дневник (журнал синхронизации)"),
    ("SUPPORT_WORKER", "support-worker", "переписка с поддержкой"),
    ("MAIN_FLOW", "main-flow", "push-подписки и расписание"),
    ("BUG_REPORT_WORKER", "bug-report-worker", "баг-репорты"),
    ("OCR_QUEUE", "ocr-queue", "задания распознавания"),
];

/// POST `https://{host}{path}` over a service binding with the shared internal key.
/// Returns the parsed body on 2xx, or a human-readable reason — never a silent skip.
async fn binding_call(
    env: &Env,
    binding: &str,
    host: &str,
    path: &str,
    body: &serde_json::Value,
) -> std::result::Result<serde_json::Value, String> {
    let key = token::secret_or_var(env, "INTERNAL_PUSH_KEY")
        .await
        .map_err(|e| format!("internal key: {e}"))?;
    let headers = Headers::new();
    headers.set("Content-Type", "application/json").map_err(|e| format!("{e}"))?;
    headers.set("X-Internal-Key", &key).map_err(|e| format!("{e}"))?;
    let mut init = RequestInit::new();
    init.with_method(Method::Post)
        .with_headers(headers)
        .with_body(Some(JsValue::from_str(&body.to_string())));
    let request = Request::new_with_init(&format!("https://{host}{path}"), &init)
        .map_err(|e| format!("build request: {e}"))?;
    let svc = env.service(binding).map_err(|e| format!("binding {binding}: {e}"))?;
    let mut res = svc
        .fetch_request(request)
        .await
        .map_err(|e| format!("call {binding}: {e}"))?;
    let status = res.status_code();
    let text = res.text().await.map_err(|e| format!("read {binding}: {e}"))?;
    if !(200..300).contains(&status) {
        return Err(format!("HTTP {status}: {}", text.chars().take(200).collect::<String>()));
    }
    serde_json::from_str(&text).map_err(|e| format!("parse {binding}: {e}"))
}

/// GET /admin/users — ОДНА строка на пользователя (не на платёж): счётчики
/// оплат и инвойсов, времена, есть ли у него ключ. Заменяет список «оплатили,
/// нет доступа», который дублировал пользователя на каждый платёж.
async fn admin_users(env: &Env) -> Result<Response> {
    let claim = claim_stub(env)?;
    let mut r = do_get(&claim, "/users-summary").await?;
    let v: serde_json::Value = r.json().await?;
    let empty = vec![];
    let users = v.get("users").and_then(|x| x.as_array()).unwrap_or(&empty);
    let mut out: Vec<serde_json::Value> = Vec::with_capacity(users.len());
    for u in users {
        let uid = u.get("user_id").and_then(|x| x.as_str()).unwrap_or("");
        if uid.is_empty() {
            continue;
        }
        let mut row = u.clone();
        // None → «неизвестно»: показываем как есть, а не прячем строку.
        let has_key = auth_has_credentials(env, uid).await;
        row["has_credentials"] = match has_key {
            Some(b) => serde_json::json!(b),
            None => serde_json::Value::Null,
        };
        out.push(row);
    }
    Response::from_json(&serde_json::json!({ "users": out }))
}

/// POST /admin/notify-cancelled — разовая рассылка вдогонку.
///
/// Кому: у кого подписка помечена отменённой ИЛИ оплаченный период уже кончился, при
/// этом в журнале уведомлений об отмене для него НЕТ записи. То есть ровно тем, кого
/// мы потеряли молча, и никому больше — повторно человек сообщение не получит ни от
/// этой ручки, ни от вебхука: журнал один на все пути.
///
/// `{"dryRun": true}` (значение по умолчанию) — только список, без единой отправки.
async fn admin_notify_cancelled(env: &Env, body: &serde_json::Value) -> Result<Response> {
    let dry_run = body.get("dryRun").and_then(|v| v.as_bool()).unwrap_or(true);
    // Каждый пользователь стоит двух-трёх обращений к DO, а на воркер их отпущено
    // ограниченное число — поэтому идём страницами, а не всем списком сразу.
    // `scanned`/`nextOffset` в ответе говорят, докуда дошли и с чего продолжить.
    let offset = body.get("offset").and_then(|v| v.as_u64()).unwrap_or(0) as usize;
    // 50 — из замера на деве: страница в 100 человек занимает ~18 с, что уже близко к
    // потолку, а 50 укладывается в 6 с.
    let limit = body.get("limit").and_then(|v| v.as_u64()).unwrap_or(50).clamp(1, 200) as usize;

    let claim = claim_stub(env)?;
    let mut r = do_get(&claim, "/users-summary").await?;
    let v: serde_json::Value = r.json().await?;
    let empty = vec![];
    let all = v.get("users").and_then(|x| x.as_array()).unwrap_or(&empty);
    let total = all.len();
    let users: Vec<&serde_json::Value> = all.iter().skip(offset).take(limit).collect();
    let scanned = users.len();

    let mut out: Vec<serde_json::Value> = Vec::new();
    let mut sent_count = 0usize;
    for u in users {
        let uid = u.get("user_id").and_then(|x| x.as_str()).unwrap_or("");
        if uid.is_empty() {
            continue;
        }
        let sub = sub_stub(env, uid)?;
        let mut sr = do_get(&sub, "/subscription").await?;
        let st: serde_json::Value = sr.json().await.unwrap_or(serde_json::json!({}));
        let end = st.get("end").and_then(|x| x.as_i64()).unwrap_or(0);
        let active = st.get("active").and_then(|x| x.as_bool()).unwrap_or(false);
        let status = st.get("status").and_then(|x| x.as_str()).unwrap_or("");
        // Никогда не плативший (end == 0) — не «потерянный подписчик», его не трогаем.
        let lost = end > 0 && (!active || status == "cancelled");
        if !lost {
            continue;
        }
        let mut nr = do_post(
            &claim,
            "/notice/sent",
            &serde_json::json!({ "userId": uid, "kind": "cancelled" }),
        )
        .await?;
        let nv: serde_json::Value = nr.json().await.unwrap_or(serde_json::json!({}));
        if nv.get("sent").and_then(|x| x.as_bool()).unwrap_or(false) {
            continue;
        }
        let tg = tg_of_user(env, uid)
            .await
            .or_else(|| u.get("tg_user_id").and_then(|x| x.as_i64()));
        let days_left = days_left_until(end);
        let mut row = serde_json::json!({
            "userId": uid,
            "tgUserId": tg,
            "status": status,
            "end": end,
            "daysLeft": days_left,
            "sent": false,
        });
        if tg.is_none() {
            row["skipped"] = serde_json::json!("no_telegram");
        } else if !dry_run {
            notify_cancelled(env, Some(uid), tg, &format!("backfill:{uid}"), days_left).await;
            row["sent"] = serde_json::json!(true);
            sent_count += 1;
        }
        out.push(row);
    }
    let next_offset = offset + scanned;
    Response::from_json(&serde_json::json!({
        "dryRun": dry_run,
        "totalUsers": total,
        "scanned": scanned,
        "nextOffset": if next_offset < total { serde_json::json!(next_offset) } else { serde_json::Value::Null },
        "candidates": out.len(),
        "sent": sent_count,
        "users": out,
    }))
}

/// GET /admin/user-card?user_id= — who exactly is about to be erased: account,
/// passkeys and tokens with their timestamps (auth-worker) plus the payment facts
/// owned here. A failed lookup is reported in the payload, never hidden.
async fn admin_user_card(env: &Env, user_id: &str) -> Result<Response> {
    if user_id.is_empty() {
        return Ok(error_response("missing user_id", 400));
    }
    let (auth, auth_error) = match binding_call(
        env,
        "AUTH_WORKER",
        "auth-worker",
        "/internal/user-card",
        &serde_json::json!({ "userId": user_id }),
    )
    .await
    {
        Ok(v) => (Some(v), None),
        Err(e) => {
            console_error!("user-card {user_id}: auth-worker: {e}");
            (None, Some(e))
        }
    };

    let sub: serde_json::Value = {
        let stub = sub_stub(env, user_id)?;
        let mut r = do_get(&stub, "/subscription").await?;
        r.json().await?
    };
    let claims: serde_json::Value = {
        let stub = claim_stub(env)?;
        let mut r = do_post(&stub, "/by-user", &serde_json::json!({ "user_id": user_id })).await?;
        r.json().await?
    };

    let receipts: serde_json::Value = {
        let stub = claim_stub(env)?;
        let mut r = do_post(&stub, "/receipts-by-user", &serde_json::json!({ "user_id": user_id }))
            .await?;
        r.json().await?
    };

    Response::from_json(&serde_json::json!({
        "user_id": user_id,
        "auth": auth,
        "auth_error": auth_error,
        "subscription": sub,
        // Все обращения к оплате: и удачные, и висящие инвойсы, и аннулированные —
        // по ним видно, что у пользователя пошло не так.
        "claims": claims.get("claims").cloned().unwrap_or(serde_json::json!([])),
        "receipts": receipts.get("receipts").cloned().unwrap_or(serde_json::json!([])),
    }))
}

/// POST /admin/user-reset {userId} — вернуть пользователя в состояние «сразу
/// после оплаты»: снять доступ (ключи, токены, коды, отметки открытых глав),
/// оставив нетронутыми и деньги, и личные данные. После этого мини-апп снова
/// рисует «Получить доступ к re:Norma», и онбординг проходится честно.
async fn admin_user_reset(env: &Env, body: &serde_json::Value) -> Result<Response> {
    let user_id = body
        .get("userId")
        .or_else(|| body.get("user_id"))
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    if user_id.is_empty() {
        return Ok(error_response("missing userId", 400));
    }
    let mut steps: Vec<serde_json::Value> = Vec::new();
    let mut failed = false;
    match binding_call(
        env,
        "AUTH_WORKER",
        "auth-worker",
        "/internal/user-reset-access",
        &serde_json::json!({ "userId": user_id }),
    )
    .await
    {
        Ok(v) => steps.push(serde_json::json!({
            "step": "доступ: ключи, токены, коды, отметки глав", "ok": true, "info": v,
        })),
        Err(e) => {
            console_error!("user-reset {user_id}: auth: {e}");
            failed = true;
            steps.push(serde_json::json!({
                "step": "доступ: ключи, токены, коды, отметки глав", "ok": false, "error": e,
            }));
        }
    }
    // Ничего больше не трогаем сознательно: платежи, подписка и чеки остаются,
    // чтобы кнопка в мини-аппе была доступна; дневник и прогресс — тоже.
    steps.push(serde_json::json!({
        "step": "платежи, подписка и чеки", "ok": true, "info": { "kept": true },
    }));
    steps.push(serde_json::json!({
        "step": "личные данные (дневник, истории, переписка)", "ok": true, "info": { "kept": true },
    }));

    console_log!("user-reset {user_id}: failed={failed}");
    Ok(Response::from_json(&serde_json::json!({
        "ok": !failed, "user_id": user_id, "steps": steps,
    }))?
    .with_status(if failed { 207 } else { 200 }))
}

/// POST /admin/user-wipe {userId} — erase the account everywhere, as if it had never
/// existed. Order matters: the provider subscription is cancelled FIRST (otherwise it
/// keeps charging); only then is local state dropped, and only if that cancellation
/// succeeded. Every step reports its own ok/error — a partial wipe never reads as
/// success (207).
async fn admin_user_wipe(env: &Env, body: &serde_json::Value) -> Result<Response> {
    let user_id = body
        .get("userId")
        .or_else(|| body.get("user_id"))
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    if user_id.is_empty() {
        return Ok(error_response("missing userId", 400));
    }
    let mut steps: Vec<serde_json::Value> = Vec::new();
    let mut failed = false;

    // 1. Provider subscription — the one thing that keeps taking money.
    let sub_status: serde_json::Value = {
        let stub = sub_stub(env, &user_id)?;
        let mut r = do_get(&stub, "/subscription").await?;
        r.json().await?
    };
    let contract = sub_status
        .get("contractId")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    if contract.is_empty() {
        steps.push(serde_json::json!({
            "step": "отмена подписки у провайдера", "ok": true,
            "info": { "skipped": "контракт не привязан" },
        }));
    } else {
        let email = sub_status
            .get("email")
            .and_then(|v| v.as_str())
            .filter(|e| !e.is_empty())
            .map(|e| e.to_string())
            .unwrap_or_else(|| format!("{user_id}@users.renorma.app"));
        match provider_for_env("lava", env).await {
            Err(reason) => {
                console_error!("user-wipe {user_id}: provider: {reason}");
                failed = true;
                steps.push(serde_json::json!({
                    "step": "отмена подписки у провайдера", "ok": false,
                    "error": format!("провайдер не сконфигурирован: {reason}"),
                }));
            }
            Ok(Some(p)) if p.configured() => match p.cancel(&contract, &email).await {
                Ok(()) => steps.push(serde_json::json!({
                    "step": "отмена подписки у провайдера", "ok": true,
                    "info": { "contract": contract },
                })),
                Err(e) => {
                    console_error!("user-wipe {user_id}: lava cancel: {e}");
                    failed = true;
                    steps.push(serde_json::json!({
                        "step": "отмена подписки у провайдера", "ok": false,
                        "error": format!("lava: {e}"),
                    }));
                }
            },
            // No provider configured means the subscription was NOT cancelled — that
            // must stop the wipe, not pass as success.
            Ok(_) => {
                console_error!("user-wipe {user_id}: lava provider unavailable");
                failed = true;
                steps.push(serde_json::json!({
                    "step": "отмена подписки у провайдера", "ok": false,
                    "error": "провайдер lava недоступен — отмена не выполнена",
                }));
            }
        }
    }
    // Money safety: a subscription that is still alive must keep its local trace,
    // so the operator can see it and retry.
    if failed {
        return Ok(Response::from_json(&serde_json::json!({
            "ok": false,
            "user_id": user_id,
            "steps": steps,
            "error": "подписка не отменена — обнуление остановлено",
        }))?
        .with_status(502));
    }

    // 2. This worker's own stores.
    for (label, which, payload) in [
        ("подписка", "sub", serde_json::json!({})),
        ("платежи и чеки", "claims", serde_json::json!({ "user_id": user_id })),
        ("индекс контрактов", "index", serde_json::json!({ "userId": user_id })),
        ("расход нейронов", "usage", serde_json::json!({ "user_id": user_id })),
    ] {
        let outcome: std::result::Result<serde_json::Value, String> = async {
            let (stub, path) = match which {
                "sub" => (sub_stub(env, &user_id).map_err(|e| e.to_string())?, "/wipe"),
                "claims" => (claim_stub(env).map_err(|e| e.to_string())?, "/wipe-user"),
                "index" => (index_stub(env).map_err(|e| e.to_string())?, "/forget-user"),
                _ => (usage_stub(env).map_err(|e| e.to_string())?, "/wipe-user"),
            };
            let mut r = do_post(&stub, path, &payload).await.map_err(|e| e.to_string())?;
            let status = r.status_code();
            let text = r.text().await.map_err(|e| e.to_string())?;
            if !(200..300).contains(&status) {
                return Err(format!("HTTP {status}: {text}"));
            }
            serde_json::from_str(&text).map_err(|e| format!("parse: {e}"))
        }
        .await;
        match outcome {
            Ok(v) => steps.push(serde_json::json!({ "step": label, "ok": true, "info": v })),
            Err(e) => {
                console_error!("user-wipe {user_id}: {label}: {e}");
                failed = true;
                steps.push(serde_json::json!({ "step": label, "ok": false, "error": e }));
            }
        }
    }

    // 3. Every other worker that owns per-user data.
    for (binding, host, label) in WIPE_TARGETS {
        // sync-worker addresses its per-user DO from the query string.
        let path = if *binding == "SYNC_WORKER" {
            format!("/internal/user-wipe?user_id={user_id}")
        } else {
            "/internal/user-wipe".to_string()
        };
        match binding_call(env, binding, host, &path, &serde_json::json!({ "userId": user_id }))
            .await
        {
            Ok(v) => steps.push(serde_json::json!({ "step": label, "ok": true, "info": v })),
            Err(e) => {
                console_error!("user-wipe {user_id}: {label}: {e}");
                failed = true;
                steps.push(serde_json::json!({ "step": label, "ok": false, "error": e }));
            }
        }
    }

    console_log!("user-wipe {user_id}: failed={failed}");
    Ok(Response::from_json(&serde_json::json!({
        "ok": !failed, "user_id": user_id, "steps": steps,
    }))?
    .with_status(if failed { 207 } else { 200 }))
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
    let i64f = |k: &str| body.get(k).and_then(|v| v.as_i64()).unwrap_or(0).max(0);
    let in_tokens = i64f("inTokens");
    let out_tokens = i64f("outTokens");
    let in_neurons = i64f("inNeurons");
    let out_neurons = i64f("outNeurons");
    let source = match body.get("source").and_then(|v| v.as_str()) {
        Some("vision") => "vision",
        Some("thirdparty") => "thirdparty",
        _ => "text",
    };
    // Модель нужна там, где счёт идёт по токенам конкретной модели (сторонний
    // провайдер). У Workers AI её место занимают нейроны, и поле может быть пустым.
    let model = body.get("model").and_then(|v| v.as_str()).unwrap_or("").trim();
    // No-op (still 200) on nothing to record — a well-formed but empty report.
    if user_id.is_empty() || in_tokens + out_tokens <= 0 {
        return Response::from_json(&serde_json::json!({ "ok": true }));
    }

    let stub = usage_stub(env)?;
    do_post(
        &stub,
        "/add",
        &serde_json::json!({
            "userId": user_id, "source": source, "model": model,
            "inTokens": in_tokens, "outTokens": out_tokens,
            "inNeurons": in_neurons, "outNeurons": out_neurons,
        }),
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
    let owner = cv.get("userId").and_then(|v| v.as_str()).map(String::from);
    // Телеграм берём с самого платежа; если письмо пришло на адрес claim'а, который
    // человек уже привязал к аккаунту, — уточняем по аккаунту (там свежее).
    let mut tg_user_id = cv.get("tgUserId").and_then(|v| v.as_i64());
    if let Some(uid) = owner.as_deref() {
        if let Some(fresh) = tg_of_user(env, uid).await {
            tg_user_id = Some(fresh);
        }
    }

    // НЕ ВСЯКОЕ ПИСЬМО ОТ LAVA — ЧЕК. Про сорванное продление она сообщает только
    // письмом (вебхука об этом мы ни разу не видели), и это единственный момент, когда
    // мы вообще узнаём о проблеме. `kind` проставляет receipt-worker по теме письма.
    let kind = body.get("kind").and_then(|v| v.as_str()).unwrap_or("");
    let msg_ref = body
        .get("messageId")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .unwrap_or(email.as_str())
        .to_string();
    match kind {
        "renewal_failed" => {
            console_warn!("письмо lava: не удалось продлить подписку, claim={claim_id}");
            notify_renewal_failed(env, owner.as_deref(), tg_user_id, &msg_ref).await;
        }
        "cancelled" => {
            console_warn!("письмо lava: подписка отменена, claim={claim_id}");
            let days_left = match owner.as_deref() {
                Some(uid) => {
                    let sub = sub_stub(env, uid)?;
                    let mut sr = do_get(&sub, "/subscription").await?;
                    let sv: serde_json::Value = sr.json().await.unwrap_or(serde_json::json!({}));
                    days_left_until(sv.get("end").and_then(|v| v.as_i64()).unwrap_or(0))
                }
                None => 0,
            };
            notify_cancelled(env, owner.as_deref(), tg_user_id, &msg_ref, days_left).await;
        }
        _ => {}
    }

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

#[cfg(test)]
mod texts_tests {
    use super::{cancelled_text, ru_days};

    /// Про причину говорим только там, где знаем её сами: письмо lava об отмене
    /// одинаково и для «кончились попытки списания», и для «человек нажал отмену».
    #[test]
    fn prichinu_nazyvaem_tolko_kogda_znaem() {
        assert!(cancelled_text(0, true).contains("продлить её не удалось"));
        assert!(!cancelled_text(0, false).contains("не удалось"));
    }

    /// Списание идёт по окончании оплаченного периода, поэтому у отмены «по неоплате»
    /// остатка дней обычно нет — и обещать его нельзя.
    #[test]
    fn ne_obeshchaem_dostup_kotorogo_net() {
        assert!(cancelled_text(0, true).contains("Доступ к приложению закрыт"));
        assert!(cancelled_text(12, false).contains("сохранится ещё 12 дней"));
    }

    #[test]
    fn dni_sklonyayutsya() {
        assert_eq!(ru_days(1), "день");
        assert_eq!(ru_days(3), "дня");
        assert_eq!(ru_days(5), "дней");
        assert_eq!(ru_days(11), "дней");
        assert_eq!(ru_days(21), "день");
        assert_eq!(ru_days(22), "дня");
    }
}

// ── Сообщения человеку о судьбе его подписки ─────────────────────────────────
//
// Раньше об отвалившемся продлении человеку писала ТОЛЬКО lava, а мы молчали: ветка
// «неудачное списание» в обработчике вебхука была пустой. Человек узнавал о потере
// доступа, открыв приложение. Эти тексты закрывают дыру.

/// Не прошло очередное списание. Текст заказчика, дословно; правлена только типографика.
const MSG_RENEWAL_FAILED: &str = "Не удалось продлить подписку на re:Norma. \
Возможно, какие-то проблемы со стороны банка или недостаточный баланс на счёте. \
В течение пары дней платёжная система будет пытаться продлить подписку. Если у неё это \
не получится, она её отменит и вы потеряете доступ к приложению. Вы сможете возобновить \
доступ, когда возобновите подписку.";

/// Об одном и том же (не прошло списание) нам говорят ДВА канала — письмо от lava и её
/// вебхук. Сутки с запасом: в это окно второй канал молчит, и человек получает одно
/// сообщение, а не два.
const NOTICE_COOLDOWN_MS: i64 = 20 * 3_600_000;

/// Как давно должен быть виден срыв продления, чтобы считать отмену его следствием.
/// Лесенка lava: попытка, повтор через 8 часов, ещё один через сутки, потом отмена —
/// то есть между первым отказом и отменой проходит меньше трёх суток. Неделя взята с
/// запасом на её задержки.
const FAILURE_LOOKBACK_MS: i64 = 7 * 86_400_000;

/// Текст об отмене.
///
/// ПРИЧИНУ НАЗЫВАЕМ ТОЛЬКО ТОГДА, КОГДА ЗНАЕМ ЕЁ САМИ. По письму lava это не
/// определить: и подписку, закрытую после неудачных списаний, и отменённую человеком
/// вручную она описывает одним шаблоном — «Вы отменили подписку». Единственный
/// надёжный признак — наша собственная запись о том, что перед этим у человека
/// сорвалось продление.
///
/// Доступ обычно доживает до конца оплаченного периода. Но списание идёт как раз по
/// его окончании, поэтому у отмены «по неоплате» дней в остатке обычно нет — и врать
/// про «ещё N дней» в этом случае нельзя.
fn cancelled_text(days_left: i64, after_failure: bool) -> String {
    let head = if after_failure {
        "Подписка на re:Norma отменена: продлить её не удалось."
    } else {
        "Подписка на re:Norma отменена."
    };
    if days_left > 0 {
        format!("{head} Доступ к приложению сохранится ещё {days_left} {}.", ru_days(days_left))
    } else {
        format!("{head} Доступ к приложению закрыт.")
    }
}

fn ru_days(n: i64) -> &'static str {
    let (t, h) = ((n % 10) as i64, (n % 100) as i64);
    if t == 1 && h != 11 {
        "день"
    } else if (2..=4).contains(&t) && !(12..=14).contains(&h) {
        "дня"
    } else {
        "дней"
    }
}

/// Сколько суток доступа осталось до `end_ms` (округляя вверх, не ниже нуля).
fn days_left_until(end_ms: i64) -> i64 {
    let day = 86_400_000i64;
    (((end_ms - Date::now().as_millis() as i64) + day - 1) / day).max(0)
}

/// Telegram-аккаунт, привязанный к учётной записи (через оплаченный claim).
///
/// Каждая осечка — вслух: молчаливый `None` здесь означает «человеку не написали», а
/// именно эту тишину мы и разбираем.
async fn tg_of_user(env: &Env, user_id: &str) -> Option<i64> {
    let claim = match claim_stub(env) {
        Ok(c) => c,
        Err(e) => {
            console_error!("tg_of_user {user_id}: CLAIM_DO: {e}");
            return None;
        }
    };
    let mut r = match do_post(&claim, "/tg-for-user", &serde_json::json!({ "userId": user_id })).await
    {
        Ok(r) => r,
        Err(e) => {
            console_error!("tg_of_user {user_id}: запрос: {e}");
            return None;
        }
    };
    let v: serde_json::Value = match r.json().await {
        Ok(v) => v,
        Err(e) => {
            console_error!("tg_of_user {user_id}: разбор ответа: {e}");
            return None;
        }
    };
    let tg = v.get("tgUserId").and_then(|x| x.as_i64());
    if tg.is_none() {
        console_warn!("tg_of_user {user_id}: телеграм не привязан — написать некуда");
    }
    tg
}

/// Занять право написать человеку. `false` — уже писали (дубль события или тот же
/// текст в окне `window_ms`); тогда отправлять НЕЛЬЗЯ. Ошибка резервирования тоже даёт
/// `false`: лучше промолчать, чем написать дважды.
async fn notice_reserve(
    env: &Env,
    id: &str,
    kind: &str,
    user_id: Option<&str>,
    ref_id: Option<&str>,
    window_ms: i64,
) -> bool {
    let claim = match claim_stub(env) {
        Ok(c) => c,
        Err(e) => {
            console_error!("notice_reserve: CLAIM_DO: {e}");
            return false;
        }
    };
    let mut r = match do_post(
        &claim,
        "/notice/reserve",
        &serde_json::json!({
            "id": id, "kind": kind, "userId": user_id, "refId": ref_id, "windowMs": window_ms,
        }),
    )
    .await
    {
        Ok(r) => r,
        Err(e) => {
            console_error!("notice_reserve {kind}/{id}: {e}");
            return false;
        }
    };
    let v: serde_json::Value = r.json().await.unwrap_or(serde_json::json!({}));
    let ok = v.get("reserved").and_then(|x| x.as_bool()).unwrap_or(false);
    if !ok {
        let reason = v.get("reason").and_then(|x| x.as_str()).unwrap_or("?");
        console_log!("notice {kind}/{id}: не отправляем ({reason})");
    }
    ok
}

/// Снять бронь уведомления — отправка не удалась, пусть следующая попытка сработает.
async fn notice_release(env: &Env, id: &str) {
    let Ok(claim) = claim_stub(env) else { return };
    if let Err(e) = do_post(&claim, "/notice/release", &serde_json::json!({ "id": id })).await {
        console_error!("notice_release {id}: {e}");
    }
}

/// Отправить произвольный текст в бот. Best-effort: неудача логируется, наверх не
/// поднимается — уведомление никогда не должно ронять обработку платежа.
async fn tg_send(env: &Env, tg_user_id: i64, text: &str) -> bool {
    let key = match token::secret_or_var(env, "INTERNAL_PUSH_KEY").await {
        Ok(k) if !k.is_empty() => k,
        _ => {
            console_error!("tg_send: INTERNAL_PUSH_KEY не настроен");
            return false;
        }
    };
    let payload = serde_json::json!({ "tgUserId": tg_user_id, "text": text }).to_string();
    let headers = Headers::new();
    let _ = headers.set("Content-Type", "application/json");
    let _ = headers.set("X-Internal-Key", &key);
    let mut init = RequestInit::new();
    init.with_method(Method::Post)
        .with_headers(headers)
        .with_body(Some(JsValue::from_str(&payload)));
    let request = match Request::new_with_init("https://telegram-worker/internal/send", &init) {
        Ok(r) => r,
        Err(e) => {
            console_error!("tg_send: сборка запроса: {e}");
            return false;
        }
    };
    let tg = match env.service("TELEGRAM_WORKER") {
        Ok(s) => s,
        Err(e) => {
            console_error!("tg_send: TELEGRAM_WORKER binding: {e}");
            return false;
        }
    };
    match tg.fetch_request(request).await {
        Ok(mut res) => {
            let sc = res.status_code();
            if (200..300).contains(&sc) {
                true
            } else {
                let t = res.text().await.unwrap_or_default();
                console_error!("tg_send: {sc} {t}");
                false
            }
        }
        Err(e) => {
            console_error!("tg_send failed: {e}");
            false
        }
    }
}

/// Отметить в журнале САМ ФАКТ сорванного продления — отдельно от того, удалось ли
/// написать человеку. Именно по этой отметке потом решается, называть ли причину
/// отмены: у lava в письме её нет, а у нас есть.
async fn record_failure_seen(env: &Env, user_id: Option<&str>, ref_id: &str) {
    let Some(uid) = user_id else { return };
    notice_reserve(
        env,
        &format!("renewal_failed_seen:{ref_id}"),
        "renewal_failed_seen",
        Some(uid),
        Some(ref_id),
        0,
    )
    .await;
}

/// Был ли у человека срыв продления в последние дни — то есть отмена пришла «по
/// неоплате», а не по его собственной кнопке.
async fn failed_recently(env: &Env, user_id: &str) -> bool {
    let Ok(claim) = claim_stub(env) else { return false };
    let Ok(mut r) = do_post(
        &claim,
        "/notice/sent",
        &serde_json::json!({ "userId": user_id, "kind": "renewal_failed_seen" }),
    )
    .await
    else {
        return false;
    };
    let v: serde_json::Value = r.json().await.unwrap_or(serde_json::json!({}));
    v.get("lastAt")
        .and_then(|x| x.as_i64())
        .map(|t| Date::now().as_millis() as i64 - t < FAILURE_LOOKBACK_MS)
        .unwrap_or(false)
}

/// «Не удалось продлить» — по письму от lava или по её вебхуку, что придёт первым.
/// `ref_id` — id письма либо ключ вебхука: по нему повтор того же события отсекается.
async fn notify_renewal_failed(
    env: &Env,
    user_id: Option<&str>,
    tg_user_id: Option<i64>,
    ref_id: &str,
) {
    // Факт запоминаем ВСЕГДА — даже если писать некуда: он нужен для текста об отмене.
    record_failure_seen(env, user_id, ref_id).await;
    let Some(tg) = tg_user_id else { return };
    let id = format!("renewal_failed:{ref_id}");
    if !notice_reserve(env, &id, "renewal_failed", user_id, Some(ref_id), NOTICE_COOLDOWN_MS).await
    {
        return;
    }
    if !tg_send(env, tg, MSG_RENEWAL_FAILED).await {
        notice_release(env, &id).await;
    }
}

/// «Подписка отменена» — по вебхуку об отмене, письму lava или разовой рассылке
/// вдогонку. Один и тот же журнал уведомлений на все три пути.
async fn notify_cancelled(
    env: &Env,
    user_id: Option<&str>,
    tg_user_id: Option<i64>,
    ref_id: &str,
    days_left: i64,
) {
    let Some(tg) = tg_user_id else { return };
    let id = format!("cancelled:{ref_id}");
    if !notice_reserve(env, &id, "cancelled", user_id, Some(ref_id), NOTICE_COOLDOWN_MS).await {
        return;
    }
    let after_failure = match user_id {
        Some(uid) => failed_recently(env, uid).await,
        None => false,
    };
    if !tg_send(env, tg, &cancelled_text(days_left, after_failure)).await {
        notice_release(env, &id).await;
    }
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
    let days_left = days_left_until(end_ms);
    // В журнал — чтобы разовая рассылка вдогонку не написала «подписка отменена»
    // человеку, который отменил её сам и уже получил сообщение.
    let notice_id = format!("cancelled:self:{user_id}:{end_ms}");
    if !notice_reserve(env, &notice_id, "cancelled", Some(user_id), None, NOTICE_COOLDOWN_MS).await
    {
        return;
    }

    let key = match token::secret_or_var(env, "INTERNAL_PUSH_KEY").await {
        Ok(k) if !k.is_empty() => k,
        _ => {
            console_warn!("notify_bot_cancelled: INTERNAL_PUSH_KEY not configured — skipping");
            notice_release(env, &notice_id).await;
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
            notice_release(env, &notice_id).await;
            return;
        }
    };
    let tg = match env.service("TELEGRAM_WORKER") {
        Ok(s) => s,
        Err(e) => {
            console_error!("notify_bot_cancelled: TELEGRAM_WORKER binding: {e}");
            notice_release(env, &notice_id).await;
            return;
        }
    };
    match tg.fetch_request(request).await {
        Ok(mut res) => {
            let sc = res.status_code();
            if !(200..300).contains(&sc) {
                let t = res.text().await.unwrap_or_default();
                console_error!("notify_bot_cancelled: {sc} {t}");
                notice_release(env, &notice_id).await;
            }
        }
        Err(e) => {
            console_error!("notify_bot_cancelled failed: {e}");
            notice_release(env, &notice_id).await;
        }
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

    // ДО любой логики: сказать в лог, ЧТО пришло, и положить тело целиком в архив.
    // Именно этого не хватило 3 сентября — событие отработало вхолостую и исчезло
    // бесследно. Архив идемпотентен по ключу события, ретрай провайдера не плодит строк.
    console_log!(
        "webhook {name}: eventType={:?} kind={} eventKey={ek} contract={:?} parent={:?} error={:?}",
        ev.event_type,
        kind_str(&ev.kind),
        ev.contract_id,
        ev.parent_contract_id,
        ev.error_message
    );
    if let Ok(claim) = claim_stub(env) {
        if let Err(e) = do_post(
            &claim,
            "/webhook-event/add",
            &serde_json::json!({
                "id": ek,
                "provider": name,
                "eventType": ev.event_type,
                "kind": kind_str(&ev.kind),
                "contractId": ev.contract_id,
                "parentContractId": ev.parent_contract_id,
                "email": ev.email,
                "errorMessage": ev.error_message,
                "payload": raw.to_string(),
            }),
        )
        .await
        {
            console_error!("webhook archive failed eventKey={ek}: {e}");
        }
    }
    if ev.kind == WebhookKind::Unknown {
        console_error!(
            "webhook {name}: НЕИЗВЕСТНЫЙ eventType={:?} — ничего не делаем, тело в архиве \
             (eventKey={ek})",
            ev.event_type
        );
    }

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
            let mut r = do_post(
                &sub,
                "/cancel",
                &serde_json::json!({ "periodEnd": ev.period_end }),
            )
            .await?;
            // Доступ живёт до конца оплаченного периода — сколько именно, знает DO,
            // а не мы: `periodEnd` в событии может отсутствовать.
            let st: serde_json::Value = r.json().await.unwrap_or(serde_json::json!({}));
            let end = st.get("end").and_then(|v| v.as_i64()).unwrap_or(0);
            let tg = tg_of_user(env, &user_id).await;
            notify_cancelled(env, Some(&user_id), tg, &ek, days_left_until(end)).await;
        }
        WebhookKind::Refunded => {
            do_post(&sub, "/refund", &serde_json::json!({})).await?;
        }
        // Не прошло списание. Состояние подписки НЕ трогаем — доступ живёт до конца
        // оплаченного периода, а lava ещё будет пытаться. Но человеку говорим сразу:
        // у него есть сутки, чтобы пополнить счёт или сменить карту.
        WebhookKind::Failed => {
            console_warn!(
                "webhook {name}: не прошло продление у user={user_id} eventType={:?} error={:?}",
                ev.event_type,
                ev.error_message
            );
            let tg = tg_of_user(env, &user_id).await;
            notify_renewal_failed(env, Some(&user_id), tg, &ek).await;
        }
        WebhookKind::Unknown => {}
    }
    Response::from_json(&serde_json::json!({ "ok": true }))
}

// Keep PROVIDER_NAMES referenced (parity with TS export; not otherwise used).
#[allow(dead_code)]
fn _provider_names() -> &'static [&'static str] {
    providers::PROVIDER_NAMES
}

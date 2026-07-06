// DEV-ONLY test double for lava.top.
//
// Lets the payment-worker's real checkout→pay→webhook path run on the TEST env
// without touching real lava.top or real money. It implements the exact lava HTTP
// contract the payment-worker calls (POST /api/v3/invoice, GET /api/v2/invoices,
// GET /api/v2/products, DELETE /api/v1/subscriptions), plus a fake hosted-checkout
// page (GET /pay) whose «Оплатить (тест)» button relays a `payment.success` webhook
// back to the payment-worker. See wrangler.toml for the money-safety rationale
// (no [env.production] → cannot deploy to prod).

use base64::Engine;
use wasm_bindgen::JsValue;
use worker::*;

mod store_do;
pub use store_do::InvoiceStoreDO;

fn json(v: serde_json::Value, status: u16) -> Result<Response> {
    Ok(Response::from_json(&v)?.with_status(status))
}

fn var(env: &Env, name: &str) -> String {
    env.var(name).map(|v| v.to_string()).unwrap_or_default()
}

/// RFC 4122 v4 UUID — the mock's contract/invoice id (the payment-worker keys the
/// webhook on this via the returned `id`).
fn uuid_v4() -> String {
    let mut b = [0u8; 16];
    getrandom::getrandom(&mut b).expect("getrandom failed");
    b[6] = (b[6] & 0x0f) | 0x40;
    b[8] = (b[8] & 0x3f) | 0x80;
    format!(
        "{:02x}{:02x}{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
        b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7],
        b[8], b[9], b[10], b[11], b[12], b[13], b[14], b[15],
    )
}

// ── DO stub ────────────────────────────────────────────────────────────────────
fn store_stub(env: &Env) -> Result<worker::durable::Stub> {
    env.durable_object("INVOICE_STORE_DO")?
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
    let headers = Headers::new();
    headers.set("Content-Type", "application/json")?;
    let mut init = RequestInit::new();
    init.with_method(Method::Post)
        .with_headers(headers)
        .with_body(Some(JsValue::from_str(&body.to_string())));
    let req = Request::new_with_init(&format!("https://do{path}"), &init)?;
    stub.fetch_with_request(req).await
}

fn api_key_ok(req: &Request, env: &Env) -> bool {
    let provided = req.headers().get("X-Api-Key").ok().flatten().unwrap_or_default();
    let expected = var(env, "MOCK_LAVA_API_KEY");
    !expected.is_empty() && provided == expected
}

/// Test price (major units) for the given currency. Same figure for every currency —
/// this is a mock; the number only needs to be plausible for the flow.
fn price(env: &Env) -> f64 {
    var(env, "LAVA_OFFER_PRICE").parse::<f64>().unwrap_or(990.0)
}

#[event(fetch)]
async fn main(mut req: Request, env: Env, _ctx: Context) -> Result<Response> {
    let url = req.url()?;
    let path = url.path().to_string();
    let method = req.method();

    match (method.clone(), path.as_str()) {
        // ── lava API surface (X-Api-Key auth, like real ApiKeyAuth) ──────────────
        (Method::Post, "/api/v3/invoice") => {
            if !api_key_ok(&req, &env) {
                return json(serde_json::json!({ "error": "unauthorized" }), 401);
            }
            let body: serde_json::Value = req.json().await.unwrap_or(serde_json::json!({}));
            let email = body.get("email").and_then(|v| v.as_str()).unwrap_or("");
            let offer_id = body.get("offerId").and_then(|v| v.as_str()).unwrap_or("");
            let currency = body.get("currency").and_then(|v| v.as_str()).unwrap_or("RUB");
            let amount = price(&env);
            let contract_id = uuid_v4();

            store_stub(&env)?;
            do_post(
                &store_stub(&env)?,
                "/invoice",
                &serde_json::json!({
                    "contractId": contract_id, "email": email,
                    "amount": amount, "currency": currency, "offerId": offer_id,
                }),
            )
            .await?;

            // Encode the price into paymentParams exactly like lava's hosted checkout,
            // so the payment-worker's decode_payment_params_amount picks it up.
            let params_json = serde_json::json!({
                "paymentSettings": { "amount_total": { "currency": currency, "amount": amount } }
            })
            .to_string();
            let params_b64 =
                base64::engine::general_purpose::STANDARD.encode(params_json.as_bytes());
            let origin = url.origin().ascii_serialization();
            let payment_url = format!(
                "{origin}/pay?pid={offer_id}&oid={contract_id}&paymentParams={params_b64}"
            );
            json(serde_json::json!({ "id": contract_id, "paymentUrl": payment_url }), 200)
        }

        (Method::Get, "/api/v2/invoices") => {
            if !api_key_ok(&req, &env) {
                return json(serde_json::json!({ "error": "unauthorized" }), 401);
            }
            let mut res = do_get(&store_stub(&env)?, "/invoices").await?;
            let v: serde_json::Value = res.json().await?;
            let empty = vec![];
            let rows = v.get("invoices").and_then(|x| x.as_array()).unwrap_or(&empty);
            let items: Vec<serde_json::Value> = rows
                .iter()
                .map(|r| {
                    let cid = r.get("contract_id").and_then(|x| x.as_str()).unwrap_or("");
                    let amt = r.get("amount").and_then(|x| x.as_f64()).unwrap_or(0.0);
                    let cur = r.get("currency").and_then(|x| x.as_str()).unwrap_or("RUB");
                    let dt = r.get("created_at").and_then(|x| x.as_i64()).unwrap_or(0);
                    serde_json::json!({
                        "id": cid,
                        "status": "COMPLETED",
                        "parentInvoice": { "id": cid },
                        "receipt": { "amount": amt, "currency": cur },
                        "datetime": iso_from_ms(dt),
                        "subscriptionStatus": "ACTIVE",
                        "subscriptionDetails": { "terminatedAt": serde_json::Value::Null },
                    })
                })
                .collect();
            json(serde_json::json!({ "total": items.len(), "items": items }), 200)
        }

        (Method::Get, "/api/v2/products") => {
            if !api_key_ok(&req, &env) {
                return json(serde_json::json!({ "error": "unauthorized" }), 401);
            }
            let amount = price(&env);
            let offer_id = var(&env, "LAVA_OFFER_ID");
            let prices: Vec<serde_json::Value> = ["RUB", "USD", "EUR"]
                .iter()
                .map(|c| serde_json::json!({ "amount": amount, "currency": c, "periodicity": "MONTHLY" }))
                .collect();
            json(
                serde_json::json!({
                    "items": [ { "offers": [ { "id": offer_id, "prices": prices } ] } ]
                }),
                200,
            )
        }

        (Method::Delete, "/api/v1/subscriptions") => {
            if !api_key_ok(&req, &env) {
                return json(serde_json::json!({ "error": "unauthorized" }), 401);
            }
            json(serde_json::json!({ "ok": true }), 200)
        }

        // ── Fake hosted-checkout page + pay action (browser-facing) ──────────────
        (Method::Get, "/pay") => serve_pay_page(&url, &env).await,
        (Method::Post, "/pay/confirm") => pay_confirm(&mut req, &env).await,

        (Method::Get, "/") => {
            Response::ok("lava-mock (dev). Endpoints: /api/v3/invoice, /api/v2/invoices, /api/v2/products, /pay")
        }
        _ => json(serde_json::json!({ "error": "not_found" }), 404),
    }
}

fn iso_from_ms(ms: i64) -> String {
    // worker Date -> ISO-8601 (RFC3339). Good enough for the mock's `datetime`.
    Date::new(DateInit::Millis(ms as u64)).to_string()
}

/// The fake hosted-checkout page. Reads the invoice by `oid`, shows the amount, and
/// a single button that POSTs /pay/confirm → relays the webhook to the payment-worker.
async fn serve_pay_page(url: &Url, env: &Env) -> Result<Response> {
    let oid = url
        .query_pairs()
        .find(|(k, _)| k == "oid")
        .map(|(_, v)| v.to_string())
        .unwrap_or_default();

    let mut res = do_get(&store_stub(env)?, &format!("/invoice?contractId={oid}")).await?;
    let (amount, currency) = if res.status_code() == 200 {
        let v: serde_json::Value = res.json().await?;
        (
            v.get("amount").and_then(|x| x.as_f64()).unwrap_or(0.0),
            v.get("currency").and_then(|x| x.as_str()).unwrap_or("RUB").to_string(),
        )
    } else {
        (0.0, "RUB".to_string())
    };
    let sym = match currency.as_str() {
        "USD" => "$",
        "EUR" => "€",
        _ => "₽",
    };

    let html = PAY_HTML
        .replace("__OID__", &oid)
        .replace("__AMOUNT__", &format!("{}", amount as i64))
        .replace("__SYM__", sym);
    let headers = Headers::new();
    headers.set("Content-Type", "text/html; charset=utf-8")?;
    headers.set("Cache-Control", "no-store")?;
    Ok(Response::ok(html)?.with_headers(headers))
}

/// Relay a `payment.success` webhook to the payment-worker for the given contract id,
/// signed with MOCK_LAVA_WEBHOOK_SECRET (== payment-worker's LAVA_WEBHOOK_SECRET).
async fn pay_confirm(req: &mut Request, env: &Env) -> Result<Response> {
    let body: serde_json::Value = req.json().await.unwrap_or(serde_json::json!({}));
    let contract_id = body.get("contractId").and_then(|v| v.as_str()).unwrap_or("");
    if contract_id.is_empty() {
        return json(serde_json::json!({ "error": "missing contractId" }), 400);
    }

    let mut inv = do_get(&store_stub(env)?, &format!("/invoice?contractId={contract_id}")).await?;
    if inv.status_code() != 200 {
        return json(serde_json::json!({ "error": "invoice_not_found" }), 404);
    }
    let iv: serde_json::Value = inv.json().await?;
    let email = iv.get("email").and_then(|x| x.as_str()).unwrap_or("");
    let amount = iv.get("amount").and_then(|x| x.as_f64()).unwrap_or(0.0);
    let currency = iv.get("currency").and_then(|x| x.as_str()).unwrap_or("RUB");

    let secret = var(env, "MOCK_LAVA_WEBHOOK_SECRET");
    let payload = serde_json::json!({
        "eventType": "payment.success",
        "contractId": contract_id,
        "buyer": { "email": email },
        "amount": amount,
        "currency": currency,
        "id": contract_id,
        "timestamp": Date::now().to_string(),
    })
    .to_string();

    let headers = Headers::new();
    headers.set("Content-Type", "application/json")?;
    headers.set("X-Api-Key", &secret)?;
    let mut init = RequestInit::new();
    init.with_method(Method::Post)
        .with_headers(headers)
        .with_body(Some(JsValue::from_str(&payload)));
    // Reach payment-worker via a service binding — same-zone worker→worker fetch to a
    // *.workers.dev URL is blocked (error 1042). The host below is a dummy; the binding
    // routes to payment-worker-dev, preserving the /webhook/lava path.
    let request = Request::new_with_init("https://payment-worker/webhook/lava", &init)?;
    let payment = env
        .service("PAYMENT_WORKER")
        .map_err(|e| Error::RustError(format!("PAYMENT_WORKER binding: {e}")))?;
    let mut relay = payment.fetch_request(request).await?;
    let status = relay.status_code();
    let text = relay.text().await.unwrap_or_default();
    json(
        serde_json::json!({ "delivered": (200..300).contains(&status), "status": status, "body": text }),
        200,
    )
}

const PAY_HTML: &str = r##"<!DOCTYPE html>
<html lang="ru"><head><meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>lava-mock — оплата</title>
<style>
  body { margin:0; font-family:-apple-system,BlinkMacSystemFont,"Segoe UI",Roboto,sans-serif;
    background:#f4f5f7; color:#0E1630; min-height:100vh; display:flex; align-items:center;
    justify-content:center; padding:24px; }
  .card { background:#fff; border-radius:18px; padding:28px; max-width:360px; width:100%;
    box-shadow:0 10px 30px rgba(14,22,48,.12); text-align:center; }
  .tag { font-size:12px; letter-spacing:.08em; text-transform:uppercase; color:#8b909a; font-weight:700; }
  .amt { font-size:40px; font-weight:800; margin:8px 0 4px; color:#10B981; }
  .warn { font-size:12px; color:#b45309; background:#fef3c7; border-radius:10px; padding:8px 12px; margin:14px 0; }
  button { width:100%; padding:16px; font-size:16px; font-weight:700; border:none; border-radius:14px;
    color:#fff; background:linear-gradient(135deg,#10B981,#059669); cursor:pointer; }
  button:disabled { opacity:.5; }
  .status { margin-top:14px; font-size:14px; min-height:20px; }
</style></head>
<body><div class="card">
  <div class="tag">lava-mock · тест</div>
  <div class="amt">__AMOUNT__ __SYM__</div>
  <div class="warn">Тестовая оплата. Реальные деньги не списываются.</div>
  <button id="pay">Оплатить (тест)</button>
  <div class="status" id="st"></div>
</div>
<script>
(function(){
  var btn=document.getElementById('pay'), st=document.getElementById('st');
  btn.addEventListener('click', function(){
    btn.disabled=true; st.textContent='Отправляем оплату…';
    fetch('/pay/confirm',{method:'POST',headers:{'Content-Type':'application/json'},
      body:JSON.stringify({contractId:'__OID__'})})
      .then(function(r){return r.json();})
      .then(function(j){
        if(j.delivered){ st.textContent='✅ Оплата проведена. Можно вернуться в приложение.'; }
        else { st.textContent='❌ Ошибка вебхука: '+(j.status||'')+' '+(j.body||''); btn.disabled=false; }
      })
      .catch(function(e){ st.textContent='❌ '+e; btn.disabled=false; });
  });
})();
</script>
</body></html>
"##;

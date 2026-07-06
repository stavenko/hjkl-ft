// lava.top provider (hosted checkout + recurring webhooks).
//
// Against the lava.top OpenAPI (gate.lava.top):
//   - POST /api/v3/invoice {email, offerId, currency} → {id, paymentUrl}.
//     `id` is the (parent) contract id and appears in EVERY webhook as
//     contractId / parentContractId — so it's our user-mapping key.
//   - Webhooks (X-Api-Key auth) carry: eventType, status, contractId,
//     parentContractId (recurring), buyer.email, willExpireAt (cancelled).
//   - DELETE /api/v1/subscriptions?contractId=&email= cancels the subscription.
//     lava has NO refund API.

use wasm_bindgen::JsCast;
use wasm_bindgen::JsValue;
use worker::*;

use super::{CheckoutOpts, WebhookEvent, WebhookKind};

pub struct Lava {
    /// API base URL. Env-driven (`LAVA_API_URL`): prod = https://gate.lava.top, dev/test =
    /// the lava-mock worker. NOT hardcoded so the test env can point at the mock.
    base: String,
    /// DEV: service binding to the lava-mock worker. Cloudflare blocks worker→worker
    /// subrequests to a `*.workers.dev` URL on the same account (error 1042), so the mock
    /// is reached via a service binding, not a plain fetch. `None` in prod → real internet
    /// fetch to gate.lava.top.
    mock: Option<Fetcher>,
    api_key: Option<String>,
    webhook_secret: Option<String>,
}

/// Result of creating a checkout: hosted-pay url + lava's contract id (orderId).
/// `amount`/`currency` are the ACTUAL amount to charge, decoded from the paymentUrl's
/// `paymentParams` (paymentSettings.amount_total). They already reflect any applied
/// promo. Both are `None` when the decode fails — FAIL-LOUD posture: we surface a
/// missing price (client shows '…') rather than fabricate a number; the invoice
/// itself remains valid + payable.
pub struct Checkout {
    pub url: String,
    pub order_id: String,
    pub amount: Option<f64>,
    pub currency: Option<String>,
}


impl Lava {
    pub fn new(
        base: String,
        mock: Option<Fetcher>,
        api_key: Option<String>,
        webhook_secret: Option<String>,
    ) -> Self {
        Self {
            base,
            mock,
            api_key,
            webhook_secret,
        }
    }

    /// Send a request to lava — via the mock service binding on dev (avoids the same-zone
    /// worker→worker fetch block), or a plain internet fetch to the real host in prod.
    async fn send(&self, req: Request) -> Result<Response> {
        match &self.mock {
            Some(f) => f.fetch_request(req).await,
            None => Fetch::Request(req).send().await,
        }
    }

    /// Credentials present — otherwise /checkout/guest returns provider_not_configured.
    pub fn configured(&self) -> bool {
        self.api_key.as_deref().map(|k| !k.is_empty()).unwrap_or(false)
    }

    pub async fn create_checkout(&self, o: &CheckoutOpts) -> Result<Checkout> {
        let api_key = self
            .api_key
            .as_deref()
            .ok_or_else(|| Error::RustError("provider_not_configured".into()))?;

        // The offer defines period/recurrence; we pass buyer + offer + the chosen currency
        // (+ acquirer channel + optional promo). The app is Russian-only, so the lava
        // checkout page is always shown in Russian, regardless of the buyer's currency.
        let buyer_language = "RU";
        let mut invoice = serde_json::json!({
            "email": o.email,
            "offerId": o.offer_id,
            "currency": o.currency,
            "buyerLanguage": buyer_language,
        });
        if let Some(pm) = &o.payment_method {
            invoice["paymentMethod"] = serde_json::json!(pm);
        }
        if let Some(pc) = &o.promo_code {
            invoice["promoCode"] = serde_json::json!(pc);
        }
        let body = invoice.to_string();

        let headers = Headers::new();
        headers
            .set("X-Api-Key", api_key)
            .map_err(|e| Error::RustError(format!("set header: {e}")))?;
        headers
            .set("Content-Type", "application/json")
            .map_err(|e| Error::RustError(format!("set header: {e}")))?;

        let mut init = RequestInit::new();
        init.with_method(Method::Post)
            .with_headers(headers)
            .with_body(Some(JsValue::from_str(&body)));
        let req = Request::new_with_init(&format!("{}/api/v3/invoice", self.base), &init)?;
        let mut res = self.send(req).await?;

        let status = res.status_code();
        if !(200..300).contains(&status) {
            // Include lava's body so the log shows WHY (e.g. invalid promo, currency not
            // priced on the offer) instead of a bare status.
            let txt = res.text().await.unwrap_or_default();
            return Err(Error::RustError(format!("lava_invoice_failed_{status}: {txt}")));
        }
        let data: serde_json::Value = res.json().await?;
        let payment_url = data.get("paymentUrl").and_then(|v| v.as_str());
        let id = data.get("id").and_then(|v| v.as_str());
        match (payment_url, id) {
            (Some(url), Some(id)) => {
                // Decode the ACTUAL price (promo-applied) from the created invoice's
                // paymentParams. A decode miss → (None, None): the invoice is still
                // valid + payable, the client just shows no price ('…').
                let (amount, currency) = match decode_payment_params_amount(url) {
                    Some((a, c)) => (Some(a), Some(c)),
                    None => (None, None),
                };
                Ok(Checkout {
                    url: url.to_string(),
                    order_id: id.to_string(),
                    amount,
                    currency,
                })
            }
            _ => Err(Error::RustError("lava_no_payment_url".into())),
        }
    }

    /// Admin reconciliation: GET /api/v2/invoices (ApiKeyAuth) — the contracts for THIS
    /// API key, across ALL statuses (default is only `completed`). Returns lava's raw
    /// page JSON verbatim so the admin sees every field (lava has no refund status, so
    /// this is how we eyeball whether a refunded contract differs at all).
    pub async fn list_invoices(&self, page: u32, size: u32) -> Result<serde_json::Value> {
        let api_key = self
            .api_key
            .as_deref()
            .ok_or_else(|| Error::RustError("provider_not_configured".into()))?;

        let url = format!(
            "{}/api/v2/invoices?page={page}&size={size}\
             &invoiceStatuses=NEW&invoiceStatuses=IN_PROGRESS\
             &invoiceStatuses=COMPLETED&invoiceStatuses=FAILED",
            self.base
        );
        let headers = Headers::new();
        headers
            .set("X-Api-Key", api_key)
            .map_err(|e| Error::RustError(format!("set header: {e}")))?;
        let mut init = RequestInit::new();
        init.with_method(Method::Get).with_headers(headers);
        let req = Request::new_with_init(&url, &init)?;
        let mut res = self.send(req).await?;

        let status = res.status_code();
        if !(200..300).contains(&status) {
            let txt = res.text().await.unwrap_or_default();
            return Err(Error::RustError(format!("lava_invoices_failed_{status}: {txt}")));
        }
        res.json().await
    }

    /// The buyer's most recent COMPLETED payment amount for a contract (the parent
    /// contract id OR any recurring child of it), read from lava's invoices. This is
    /// what they ACTUALLY paid (promo applied) — unlike the plan's list price. Returns
    /// (amount, currency); None if no matching completed invoice is found.
    pub async fn last_payment(&self, contract_id: &str) -> Result<Option<(f64, String)>> {
        let page = self.list_invoices(1, 100).await?;
        let items = match page.get("items").and_then(|v| v.as_array()) {
            Some(a) => a.clone(),
            None => return Ok(None),
        };
        let mut best_dt = String::new();
        let mut best: Option<(f64, String)> = None;
        for it in &items {
            if it.get("status").and_then(|v| v.as_str()) != Some("COMPLETED") {
                continue;
            }
            let id = it.get("id").and_then(|v| v.as_str());
            let parent = it
                .get("parentInvoice")
                .and_then(|p| p.get("id"))
                .and_then(|v| v.as_str());
            if id != Some(contract_id) && parent != Some(contract_id) {
                continue;
            }
            let amount = it
                .get("receipt")
                .and_then(|r| r.get("amount"))
                .and_then(|v| v.as_f64());
            let currency = it
                .get("receipt")
                .and_then(|r| r.get("currency"))
                .and_then(|v| v.as_str())
                .unwrap_or("RUB")
                .to_string();
            let dt = it
                .get("datetime")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            if let Some(a) = amount {
                // ISO-8601 datetimes sort lexically → keep the latest.
                if best.is_none() || dt > best_dt {
                    best_dt = dt;
                    best = Some((a, currency));
                }
            }
        }
        Ok(best)
    }

    /// The offer's LIST price for a currency, read from GET /api/v2/products WITHOUT
    /// minting an invoice. Used for the "ценник" before any promo. Response shape:
    /// { "items": [ { "offers": [ { "id": "...", "prices": [ { "amount": 200.0,
    /// "currency": "RUB", "periodicity": "..." } ] } ] } ] }. Returns (amount, currency)
    /// or None if the offer/currency isn't present (logged loudly — no fabricated price).
    pub async fn offer_price(&self, offer_id: &str, currency: &str) -> Result<Option<(f64, String)>> {
        let api_key = self
            .api_key
            .as_deref()
            .ok_or_else(|| Error::RustError("provider_not_configured".into()))?;
        let headers = Headers::new();
        headers
            .set("X-Api-Key", api_key)
            .map_err(|e| Error::RustError(format!("set header: {e}")))?;
        let mut init = RequestInit::new();
        init.with_method(Method::Get).with_headers(headers);
        let req = Request::new_with_init(&format!("{}/api/v2/products", self.base), &init)?;
        let mut res = self.send(req).await?;
        let status = res.status_code();
        if !(200..300).contains(&status) {
            let txt = res.text().await.unwrap_or_default();
            return Err(Error::RustError(format!("lava_products_failed_{status}: {txt}")));
        }
        let data: serde_json::Value = res.json().await?;
        let items = match data.get("items").and_then(|v| v.as_array()) {
            Some(a) => a,
            None => {
                console_error!("offer_price: products response has no 'items' array");
                return Ok(None);
            }
        };
        for it in items {
            let offers = match it.get("offers").and_then(|v| v.as_array()) {
                Some(o) => o,
                None => continue,
            };
            for off in offers {
                if off.get("id").and_then(|v| v.as_str()) != Some(offer_id) {
                    continue;
                }
                let prices = match off.get("prices").and_then(|v| v.as_array()) {
                    Some(p) => p,
                    None => continue,
                };
                for pr in prices {
                    if pr.get("currency").and_then(|v| v.as_str()) == Some(currency) {
                        if let Some(a) = pr.get("amount").and_then(|v| v.as_f64()) {
                            return Ok(Some((a, currency.to_string())));
                        }
                    }
                }
            }
        }
        console_error!("offer_price: offer {offer_id} / currency {currency} not found in products");
        Ok(None)
    }

    /// lava uses ApiKeyWebhookAuth → header X-Api-Key == the webhook's configured key.
    /// Returns (ok, body). Fails closed (ok=false) when no webhook_secret is configured.
    pub async fn verify_webhook(&self, req: &mut Request) -> (bool, Option<serde_json::Value>) {
        let body = req.json::<serde_json::Value>().await.ok();
        let secret = match self.webhook_secret.as_deref() {
            Some(s) => s,
            None => return (false, body),
        };
        let provided = req
            .headers()
            .get("X-Api-Key")
            .ok()
            .flatten()
            .unwrap_or_default();
        (provided == secret, body)
    }

    pub fn parse_webhook(&self, body: &serde_json::Value) -> WebhookEvent {
        let b = body;
        let event_type = b.get("eventType").and_then(|v| v.as_str()).unwrap_or("");
        let contract_id = b.get("contractId").and_then(|v| v.as_str()).map(String::from);
        let parent_contract_id = b
            .get("parentContractId")
            .and_then(|v| v.as_str())
            .map(String::from);
        let email = b
            .get("buyer")
            .and_then(|v| v.get("email"))
            .and_then(|v| v.as_str())
            .map(String::from);

        // Minor-unit amount + currency for the manual-refund record (MONEY-SAFETY #8).
        let amount = b
            .get("amount")
            .or_else(|| b.get("sum"))
            .or_else(|| b.get("price"))
            .and_then(|v| {
                if v.is_number() {
                    v.as_f64().filter(|n| n.is_finite()).map(|n| n as i64)
                } else {
                    None
                }
            });
        let currency = b.get("currency").and_then(|v| v.as_str()).map(String::from);

        // Stable id/timestamp passthroughs for a retry-stable dedup key (MONEY-SAFETY #4).
        let event_id = b
            .get("id")
            .and_then(|v| v.as_str())
            .or_else(|| b.get("eventId").and_then(|v| v.as_str()))
            .map(String::from);
        let timestamp = b
            .get("timestamp")
            .and_then(|v| v.as_str())
            .or_else(|| b.get("eventTime").and_then(|v| v.as_str()))
            .map(String::from);

        let mut period_end: Option<i64> = None;
        let kind = match event_type {
            "payment.success" => WebhookKind::Paid,
            "subscription.recurring.payment.success" => WebhookKind::Recurring,
            "subscription.cancelled" => {
                period_end = b
                    .get("willExpireAt")
                    .and_then(|v| v.as_str())
                    .and_then(parse_date_ms);
                WebhookKind::Cancelled
            }
            "payment.failed" | "subscription.recurring.payment.failed" => WebhookKind::Failed,
            "payment.refunded" | "subscription.refunded" => WebhookKind::Refunded,
            _ => WebhookKind::Failed,
        };

        WebhookEvent {
            kind,
            contract_id,
            parent_contract_id,
            email,
            period_end,
            amount,
            currency,
            event_id,
            timestamp,
        }
    }

    pub async fn cancel(&self, contract_id: &str, email: &str) -> Result<()> {
        let api_key = self
            .api_key
            .as_deref()
            .ok_or_else(|| Error::RustError("provider_not_configured".into()))?;
        let url = format!(
            "{}/api/v1/subscriptions?contractId={}&email={}",
            self.base,
            url_encode(contract_id),
            url_encode(email)
        );
        let headers = Headers::new();
        headers
            .set("X-Api-Key", api_key)
            .map_err(|e| Error::RustError(format!("set header: {e}")))?;
        let mut init = RequestInit::new();
        init.with_method(Method::Delete).with_headers(headers);
        let req = Request::new_with_init(&url, &init)?;
        let res = self.send(req).await?;
        let status = res.status_code();
        if !(200..300).contains(&status) {
            return Err(Error::RustError(format!("lava_cancel_failed_{status}")));
        }
        Ok(())
    }
}

/// Decode `paymentSettings.amount_total.{amount,currency}` from a lava paymentUrl.
///
/// The paymentUrl looks like:
///   https://app.lava.top/products/<pid>/<oid>?paymentParams=<BASE64>
/// where <BASE64> (possibly percent-encoded, possibly URL-safe) decodes to JSON:
///   { "paymentSettings": { "amount_total": { "currency": "RUB", "amount": 200.00 } } }
///
/// Steps: extract the `paymentParams=` value → percent-decode → normalise URL-safe
/// base64 (-_ → +/) → base64-decode via the JS runtime `atob` (offline, no crate) →
/// parse JSON → read amount_total. Returns `None` on any miss (FAIL-LOUD: the caller
/// records no price rather than a fabricated one). Never panics.
fn decode_payment_params_amount(payment_url: &str) -> Option<(f64, String)> {
    // (1) Pull the `paymentParams=` query value (up to the next `&`).
    let after = payment_url.split("paymentParams=").nth(1)?;
    let raw = after.split('&').next()?;
    if raw.is_empty() {
        return None;
    }

    // (2) Percent-decode (the base64 may be URL-escaped: +,/,= become %2B,%2F,%3D).
    let decoded_uri: String = match js_sys::decode_uri_component(raw) {
        Ok(js) => js.into(),
        Err(_) => raw.to_string(), // not escaped → use verbatim
    };

    // (3) Normalise URL-safe base64 (-,_ → +,/) so `atob` accepts it either way.
    let b64: String = decoded_uri.replace('-', "+").replace('_', "/");

    // (4) base64-decode via the JS runtime `atob` (no external crate → stays offline).
    //     amount_total JSON is ASCII, so atob's binary-string char codes map 1:1 to
    //     bytes and String::from(js_string) reconstructs the JSON verbatim.
    let json = atob(&b64)?;

    // (5) Parse JSON and read paymentSettings.amount_total.{amount,currency}.
    let v: serde_json::Value = serde_json::from_str(&json).ok()?;
    let total = v.get("paymentSettings")?.get("amount_total")?;
    let amount = total.get("amount").and_then(|a| a.as_f64())?;
    let currency = total.get("currency").and_then(|c| c.as_str())?.to_string();
    Some((amount, currency))
}

/// base64-decode via the Workers runtime global `atob`. Returns `None` if `atob` is
/// absent or throws (e.g. invalid base64) — no panic, FAIL-LOUD to the caller.
fn atob(b64: &str) -> Option<String> {
    let global = js_sys::global();
    let f = js_sys::Reflect::get(&global, &JsValue::from_str("atob")).ok()?;
    let func = f.dyn_ref::<js_sys::Function>()?;
    let out = func.call1(&JsValue::NULL, &JsValue::from_str(b64)).ok()?;
    out.as_string()
}

/// Parse a date string to ms epoch (NaN → None), mirroring TS `Date.parse`.
fn parse_date_ms(s: &str) -> Option<i64> {
    let ms = js_sys::Date::parse(s);
    if ms.is_nan() {
        None
    } else {
        Some(ms as i64)
    }
}

/// encodeURIComponent equivalent via the JS global.
fn url_encode(s: &str) -> String {
    js_sys::encode_uri_component(s).into()
}

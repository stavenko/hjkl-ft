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

use wasm_bindgen::JsValue;
use worker::*;

use super::{CheckoutOpts, WebhookEvent, WebhookKind};

const LAVA_API: &str = "https://gate.lava.top";

pub struct Lava {
    api_key: Option<String>,
    webhook_secret: Option<String>,
}

/// Result of creating a checkout: hosted-pay url + lava's contract id (orderId).
pub struct Checkout {
    pub url: String,
    pub order_id: String,
}

impl Lava {
    pub fn new(api_key: Option<String>, webhook_secret: Option<String>) -> Self {
        Self {
            api_key,
            webhook_secret,
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

        // The offer defines period/recurrence; we only pass buyer + offer + currency
        // (+ an optional promo code).
        let mut invoice = serde_json::json!({
            "email": o.email,
            "offerId": o.offer_id,
            "currency": "RUB",
            "buyerLanguage": "RU",
        });
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
        let req = Request::new_with_init(&format!("{LAVA_API}/api/v3/invoice"), &init)?;
        let mut res = Fetch::Request(req).send().await?;

        let status = res.status_code();
        if !(200..300).contains(&status) {
            return Err(Error::RustError(format!("lava_invoice_failed_{status}")));
        }
        let data: serde_json::Value = res.json().await?;
        let payment_url = data.get("paymentUrl").and_then(|v| v.as_str());
        let id = data.get("id").and_then(|v| v.as_str());
        match (payment_url, id) {
            (Some(url), Some(id)) => Ok(Checkout {
                url: url.to_string(),
                order_id: id.to_string(),
            }),
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
            "{LAVA_API}/api/v2/invoices?page={page}&size={size}\
             &invoiceStatuses=NEW&invoiceStatuses=IN_PROGRESS\
             &invoiceStatuses=COMPLETED&invoiceStatuses=FAILED"
        );
        let headers = Headers::new();
        headers
            .set("X-Api-Key", api_key)
            .map_err(|e| Error::RustError(format!("set header: {e}")))?;
        let mut init = RequestInit::new();
        init.with_method(Method::Get).with_headers(headers);
        let req = Request::new_with_init(&url, &init)?;
        let mut res = Fetch::Request(req).send().await?;

        let status = res.status_code();
        if !(200..300).contains(&status) {
            let txt = res.text().await.unwrap_or_default();
            return Err(Error::RustError(format!("lava_invoices_failed_{status}: {txt}")));
        }
        res.json().await
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
            "{LAVA_API}/api/v1/subscriptions?contractId={}&email={}",
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
        let res = Fetch::Request(req).send().await?;
        let status = res.status_code();
        if !(200..300).contains(&status) {
            return Err(Error::RustError(format!("lava_cancel_failed_{status}")));
        }
        Ok(())
    }
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

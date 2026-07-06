// Payment provider abstraction. lava.top is the first (and only) provider; the
// rest of the worker talks only to this normalized shape.

mod lava;

pub use lava::Lava;

/// A payment-provider webhook, normalized to what the SubscriptionDO needs.
#[derive(Debug, Clone, PartialEq)]
pub enum WebhookKind {
    Paid,
    Recurring,
    Cancelled,
    Refunded,
    Failed,
}

#[derive(Debug, Clone)]
pub struct WebhookEvent {
    pub kind: WebhookKind,
    pub contract_id: Option<String>,
    pub parent_contract_id: Option<String>,
    pub email: Option<String>,
    /// ms epoch access runs until, if the provider says (e.g. willExpireAt).
    pub period_end: Option<i64>,
    /// minor units paid (manual-refund display; MONEY-SAFETY #8).
    pub amount: Option<i64>,
    pub currency: Option<String>,
    /// provider's stable event id, if any (webhook dedup; MONEY-SAFETY #4).
    pub event_id: Option<String>,
    /// provider event timestamp passthrough (webhook dedup fallback).
    pub timestamp: Option<String>,
}

/// Options for creating a hosted checkout invoice.
pub struct CheckoutOpts {
    pub offer_id: String,
    pub email: String,
    #[allow(dead_code)]
    pub return_url: String,
    pub promo_code: Option<String>,
}

pub const PROVIDER_NAMES: &[&str] = &["lava"];

/// Resolve a provider by name, with credentials already read from the Secrets
/// Store (dev/test = None → not configured → real pay impossible).
pub fn provider_for(name: &str, api_key: Option<String>, webhook_secret: Option<String>) -> Option<Lava> {
    match name {
        "lava" => Some(Lava::new(api_key, webhook_secret)),
        _ => None,
    }
}

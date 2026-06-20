// lava.top provider (hosted checkout + recurring webhooks).
//
// Implemented against the lava.top OpenAPI (gate.lava.top/docs/documentation.yaml):
//   - POST /api/v3/invoice {email, offerId, currency} → {id, paymentUrl}.
//     `id` is the (parent) contract id and appears in EVERY webhook as
//     contractId / parentContractId — so it's our user-mapping key.
//   - Webhooks (X-Api-Key auth) carry: eventType, status, contractId,
//     parentContractId (recurring), buyer.email, willExpireAt (cancelled).
//   - DELETE /api/v1/subscriptions?contractId=&email= cancels the subscription.

import type { CheckoutOpts, PaymentProvider, ProviderEnv, WebhookEvent } from "./index";

const LAVA_API = "https://gate.lava.top";

export class LavaProvider implements PaymentProvider {
  readonly name = "lava";
  private apiKey?: string;
  private webhookSecret?: string;

  constructor(env: ProviderEnv) {
    this.apiKey = env.LAVA_API_KEY;
    this.webhookSecret = env.LAVA_WEBHOOK_SECRET;
  }

  configured(): boolean {
    return !!this.apiKey;
  }

  async createCheckout(o: CheckoutOpts): Promise<{ url: string; orderId: string }> {
    if (!this.apiKey) throw new Error("provider_not_configured");
    const res = await fetch(`${LAVA_API}/api/v3/invoice`, {
      method: "POST",
      headers: { "X-Api-Key": this.apiKey, "Content-Type": "application/json" },
      // The offer defines period/recurrence; we only pass buyer + offer + currency.
      body: JSON.stringify({
        email: o.email,
        offerId: o.offerId,
        currency: "RUB",
        buyerLanguage: "RU",
      }),
    });
    if (!res.ok) throw new Error(`lava_invoice_failed_${res.status}`);
    const data = (await res.json()) as { id?: string; paymentUrl?: string };
    if (!data.paymentUrl || !data.id) throw new Error("lava_no_payment_url");
    // orderId = lava's contract id; the webhook echoes it as contractId/parentContractId.
    return { url: data.paymentUrl, orderId: data.id };
  }

  async verifyWebhook(req: Request): Promise<{ ok: boolean; body?: unknown }> {
    const body = await req.json().catch(() => null);
    if (!this.webhookSecret) return { ok: false };
    // lava uses ApiKeyWebhookAuth → header X-Api-Key == the webhook's configured key.
    const provided = req.headers.get("X-Api-Key") ?? "";
    return { ok: provided === this.webhookSecret, body };
  }

  parseWebhook(body: unknown): WebhookEvent {
    const b = (body ?? {}) as Record<string, any>;
    const eventType = String(b.eventType ?? "");
    const contractId = b.contractId;
    const parentContractId = b.parentContractId;
    const email = b?.buyer?.email;

    let kind: WebhookEvent["kind"];
    let periodEnd: number | undefined;
    switch (eventType) {
      case "payment.success":
        kind = "paid";
        break;
      case "subscription.recurring.payment.success":
        kind = "recurring";
        break;
      case "subscription.cancelled":
        kind = "cancelled";
        periodEnd = b.willExpireAt ? Date.parse(b.willExpireAt) || undefined : undefined;
        break;
      // payment.failed / subscription.recurring.payment.failed and anything else:
      default:
        kind = "failed";
    }
    return { kind, contractId, parentContractId, email, periodEnd };
  }

  async cancel(contractId: string, email: string): Promise<void> {
    if (!this.apiKey) throw new Error("provider_not_configured");
    const url = `${LAVA_API}/api/v1/subscriptions?contractId=${encodeURIComponent(contractId)}&email=${encodeURIComponent(email)}`;
    const res = await fetch(url, { method: "DELETE", headers: { "X-Api-Key": this.apiKey } });
    if (!res.ok) throw new Error(`lava_cancel_failed_${res.status}`);
  }
}

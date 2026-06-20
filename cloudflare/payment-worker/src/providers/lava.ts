// lava.top provider (hosted checkout + recurring webhooks).
//
// Flow: createCheckout → POST an invoice to gate.lava.top → return lava's hosted
// paymentUrl (the frontend redirects there). lava then calls our webhook on
// "Payment Result" (first/regular) and "Recurring Payment" (subsequent charges),
// plus cancel/refund. We pass our own orderId so the webhook maps back to the user.
//
// NOTE: exact lava field names / endpoints / signature scheme are confirmed
// against gate.lava.top/docs when wiring real credentials — every spot that
// depends on them is marked `TODO(lava-fields)`.

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
    const orderId = "ord_" + crypto.randomUUID();

    // TODO(lava-fields): confirm endpoint + body shape (gate.lava.top/docs).
    const res = await fetch(`${LAVA_API}/api/v2/invoice`, {
      method: "POST",
      headers: { "X-Api-Key": this.apiKey, "Content-Type": "application/json" },
      body: JSON.stringify({
        offerId: o.offerId,
        email: o.email ?? `${o.userId}@users.renorma.app`,
        currency: "RUB",
        buyerLanguage: "RU",
        // Carry our orderId so the webhook can resolve the user. TODO(lava-fields):
        // confirm which echoed field to use (clientUtm / additionalData / etc.).
        clientUtm: { utm_content: orderId },
        successUrl: o.returnUrl,
        failUrl: o.returnUrl,
      }),
    });
    if (!res.ok) throw new Error(`lava_invoice_failed_${res.status}`);
    const data = (await res.json()) as Record<string, any>;
    // TODO(lava-fields): confirm the hosted-page URL field.
    const url = data.paymentUrl ?? data.url ?? data?.data?.url;
    if (!url || typeof url !== "string") throw new Error("lava_no_payment_url");
    return { url, orderId };
  }

  async verifyWebhook(req: Request): Promise<{ ok: boolean; body?: unknown }> {
    const body = await req.json().catch(() => null);
    if (!this.webhookSecret) return { ok: false };
    // TODO(lava-fields): confirm lava's webhook auth — shared secret header vs HMAC
    // over the raw body. Currently: shared secret in X-Api-Key must match.
    const provided = req.headers.get("X-Api-Key") ?? "";
    return { ok: provided === this.webhookSecret, body };
  }

  parseWebhook(body: unknown): WebhookEvent {
    const b = (body ?? {}) as Record<string, any>;
    // TODO(lava-fields): confirm status/event names and payload shape.
    const status = String(b.status ?? "").toLowerCase();
    const eventType = String(b.eventType ?? b.type ?? "").toLowerCase();
    const orderId = b?.clientUtm?.utm_content ?? b.orderId;
    const contractId = b.contractId ?? b.parentContractId;
    const planId = b.offerId;
    const periodEnd =
      typeof b.nextPayment === "string" ? Date.parse(b.nextPayment) || undefined : undefined;

    let kind: WebhookEvent["kind"] = "failed";
    if (status.includes("fail") || status.includes("declin")) kind = "failed";
    else if (eventType.includes("recurr")) kind = "recurring";
    else if (status.includes("refund")) kind = "refunded";
    else if (status.includes("cancel")) kind = "cancelled";
    else if (status.includes("success") || status.includes("paid") || status.includes("complete"))
      kind = "paid";

    return { kind, orderId, contractId, periodEnd, planId };
  }

  async cancel(contractId: string): Promise<void> {
    if (!this.apiKey) throw new Error("provider_not_configured");
    // TODO(lava-fields): confirm cancel/refund endpoint.
    await fetch(`${LAVA_API}/api/v2/subscriptions/${encodeURIComponent(contractId)}/cancel`, {
      method: "POST",
      headers: { "X-Api-Key": this.apiKey },
    });
  }
}

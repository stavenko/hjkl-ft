// Payment provider abstraction.
//
// lava.top is the first provider, but not the only one — the rest of the worker
// (SubscriptionDO, the gates, the frontend) is provider-agnostic and talks only
// to this interface. Adding a provider = one new module + a case in getProvider.

export interface CheckoutOpts {
  userId: string;
  planId: string;
  offerId: string; // provider-specific product/offer id (from the plan catalog)
  email?: string;
  returnUrl: string; // where the hosted checkout returns the buyer
}

/** A payment-provider webhook, normalized to what the SubscriptionDO needs. */
export interface WebhookEvent {
  kind: "paid" | "recurring" | "cancelled" | "refunded" | "failed";
  // The contract id (and, for recurring charges, the parent/root contract id).
  // We map back to our user via whichever was stored at checkout.
  contractId?: string;
  parentContractId?: string;
  email?: string; // buyer email lava recorded (also used for cancel)
  periodEnd?: number; // ms epoch access runs until, if the provider says (e.g. willExpireAt)
}

export interface ProviderEnv {
  LAVA_API_KEY?: string;
  LAVA_WEBHOOK_SECRET?: string;
}

export interface PaymentProvider {
  readonly name: string;
  /** Credentials present — otherwise /checkout returns provider_not_configured. */
  configured(): boolean;
  createCheckout(o: CheckoutOpts): Promise<{ url: string; orderId: string }>;
  /** Verify authenticity (signature/secret) and return the parsed body. */
  verifyWebhook(req: Request): Promise<{ ok: boolean; body?: unknown }>;
  parseWebhook(body: unknown): WebhookEvent;
  /** Cancel the recurring contract (no further charges). Optional. */
  cancel?(contractId: string, email: string): Promise<void>;
}

import { LavaProvider } from "./lava";

export const PROVIDER_NAMES = ["lava"] as const;

export function getProvider(name: string, env: ProviderEnv): PaymentProvider | null {
  switch (name) {
    case "lava":
      return new LavaProvider(env);
    default:
      return null;
  }
}

"use server";

/**
 * Server action for the buy flow. Called from the `<BuyForm>` client
 * component — wraps `createCheckout` so the Stripe-calling `lib/api`
 * (which touches Clerk server modules) stays out of client bundles.
 * Returns the hosted-checkout URL; the client redirects to it.
 */

import {
  createCheckout,
  CheckoutRefusedError,
  type ShippingAddress,
} from "@/lib/api";

/**
 * A refusal comes back as a value, not a throw: Next.js hides thrown
 * server-action messages in production, and "sold out" / "someone else
 * is checking out the last copy" are messages the buyer needs to see.
 */
export type StartCheckoutResult =
  | { ok: true; checkout_url: string; order_id: string }
  | { ok: false; message: string };

export async function startCheckout(
  artworkId: string,
  shipping: ShippingAddress
): Promise<StartCheckoutResult> {
  try {
    const res = await createCheckout(artworkId, shipping);
    return { ok: true, ...res };
  } catch (e) {
    if (e instanceof CheckoutRefusedError) return { ok: false, message: e.message };
    throw e;
  }
}

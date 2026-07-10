"use server";

/**
 * Server action for the buy flow. Called from the `<BuyForm>` client
 * component — wraps `createCheckout` so the Stripe-calling `lib/api`
 * (which touches Clerk server modules) stays out of client bundles.
 * Returns the hosted-checkout URL; the client redirects to it.
 */

import { createCheckout, type ShippingAddress } from "@/lib/api";

export async function startCheckout(
  artworkId: string,
  shipping: ShippingAddress
): Promise<{ checkout_url: string; order_id: string }> {
  return createCheckout(artworkId, shipping);
}

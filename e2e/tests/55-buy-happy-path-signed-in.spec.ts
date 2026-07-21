import { readFileSync } from "node:fs";
import { test, expect } from "@playwright/test";
import { setupClerkTestingToken } from "@clerk/testing/playwright";
import { createArtwork, enablePayouts, makeSellable } from "../lib/fixtures";

/**
 * M-05/M-10 — full buy happy path through Stripe test-mode checkout.
 *
 * SKIPPED unless STRIPE_E2E=1. This is the one marketplace spec that
 * can't run on fixtures alone: minting a Checkout Session with
 * `transfer_data[destination]` needs a *genuinely onboarded* Connect
 * account (the fixture's fake acct can't take a destination charge), and
 * confirming the payment needs `stripe listen` forwarding
 * `checkout.session.completed` back to the API. To run it:
 *
 *   1. STRIPE_SECRET_KEY = a test-mode key.
 *   2. STRIPE_E2E_ARTIST_SLUG = the slug of a fixture artist whose
 *      `stripe_account_id` is a real, fully-onboarded **test** Connect
 *      account (charges + payouts enabled). enablePayouts() only flips
 *      the DB flags; it does NOT create a chargeable Stripe account.
 *   3. `stripe listen --forward-to <api>/v1/webhooks/stripe` running.
 *   4. STRIPE_E2E=1.
 *
 * The Stripe-hosted card fields live in iframes owned by Stripe; those
 * selectors are Stripe's, not ours, and may need adjusting if Stripe
 * changes its checkout DOM.
 */
test("buy-happy-path-signed-in: checkout → paid → confirmation", async ({
  page,
}) => {
  test.skip(
    !process.env.STRIPE_E2E,
    "needs the live Stripe test-mode loop (onboarded Connect account + stripe listen)",
  );

  await setupClerkTestingToken({ page });

  // An onboarded artist is required for a real destination charge. Prefer
  // the explicitly-provided slug; fall back to the fixture artist (whose
  // account must still be a real onboarded test account for this to pass).
  const artistSlug =
    process.env.STRIPE_E2E_ARTIST_SLUG ??
    (
      JSON.parse(readFileSync("e2e/.auth/artist-meta.json", "utf8")) as {
        slug: string;
      }
    ).slug;
  await enablePayouts(artistSlug);

  const artwork = await createArtwork({
    artistSlug,
    title: "E2E buy-happy-path work",
    priceCents: 50_000,
    currency: "GBP",
    dimensions: { width_cm: 30, height_cm: 40 },
    status: "published",
  });
  await makeSellable(artwork.id);

  // 1. Buyer opens the buy form and fills the shipping address.
  await page.goto(`/artworks/${artwork.id}/buy`);
  await page.getByLabel("Recipient name").fill("E2E Buyer");
  await page.getByLabel("Address line 1").fill("1 Test Street");
  await page.getByLabel("City").fill("London");
  await page.getByLabel("Postcode").fill("EC1A 1BB");
  await page.getByLabel("Country").fill("GB");

  // 2. Continue → server action mints the Checkout Session and redirects.
  await page.getByRole("button", { name: /^Continue to payment$/ }).click();
  await page.waitForURL(/checkout\.stripe\.com/, { timeout: 30_000 });

  // 3. Pay with Stripe's test card. These fields are Stripe-owned.
  await page
    .getByRole("textbox", { name: /card number/i })
    .fill("4242424242424242");
  await page.getByRole("textbox", { name: /expir/i }).fill("12 / 34");
  await page.getByRole("textbox", { name: /cvc/i }).fill("123");
  await page.getByRole("button", { name: /pay/i }).click();

  // 4. Stripe redirects back; the webhook flips the order to paid, so the
  //    confirmation renders. (The webhook may land a beat after redirect —
  //    the generous timeout absorbs the stripe listen round-trip.)
  await page.waitForURL(/\/orders\//, { timeout: 30_000 });
  await expect(
    page.getByRole("heading", { name: /Order confirmed/i }),
  ).toBeVisible({ timeout: 30_000 });
});

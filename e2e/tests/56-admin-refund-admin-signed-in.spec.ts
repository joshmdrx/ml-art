import { test, expect } from "@playwright/test";
import { setupClerkTestingToken } from "@clerk/testing/playwright";

/**
 * M-08/M-10 — admin refunds a paid order.
 *
 * SKIPPED unless STRIPE_E2E=1. The refund endpoint calls Stripe
 * (`refunds.create`) with `reverse_transfer` + `refund_application_fee`
 * on a real payment intent, so it needs the live Stripe test-mode loop.
 * The fixture `create-order` inserts a synthetic `pi_test_*` Stripe won't
 * recognise, so this spec drives a *real* paid order instead. To run it:
 *
 *   1. STRIPE_SECRET_KEY = a test-mode key + `stripe listen` forwarding
 *      `charge.refunded` to `<api>/v1/webhooks/stripe`.
 *   2. STRIPE_E2E_ORDER_ID = the id of a real paid order (e.g. one left
 *      behind by spec 55's completed checkout).
 *   3. STRIPE_E2E=1.
 *
 * Selectors mirror `components/admin/RefundButton.tsx` (reason `<select
 * aria-label="Refund reason">`, the destructive-confirm dialog, and the
 * "Refund started" success toast).
 */
test("admin-refund-admin-signed-in: refund a paid order", async ({ page }) => {
  test.skip(
    !process.env.STRIPE_E2E,
    "needs the live Stripe test-mode loop (real paid order + stripe listen)",
  );

  const orderId = process.env.STRIPE_E2E_ORDER_ID;
  test.skip(
    !orderId,
    "set STRIPE_E2E_ORDER_ID to a real paid order (e.g. from spec 55)",
  );

  await setupClerkTestingToken({ page });

  // 1. Admin opens the paid order.
  await page.goto(`/admin/orders/${orderId}`);
  await expect(page.getByRole("button", { name: /^Refund$/ })).toBeVisible({
    timeout: 15_000,
  });

  // 2. Pick a reason + refund, then confirm the destructive dialog.
  await page.getByLabel("Refund reason").selectOption("not-as-described");
  await page.getByRole("button", { name: /^Refund$/ }).click();
  await page.getByRole("button", { name: /confirm|refund/i }).click();

  // 3. Success toast. The charge.refunded webhook (via stripe listen) then
  //    flips status → refunded and emails buyer + artist.
  await expect(page.getByText(/Refund started/)).toBeVisible({
    timeout: 15_000,
  });
});

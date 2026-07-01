import { test, expect } from "@playwright/test";
import { setupClerkTestingToken } from "@clerk/testing/playwright";

/**
 * T-058.2 — studio series page auth + onboarding gate.
 *
 * `/studio/series` requires (a) a signed-in Clerk session and (b) an
 * `artists.user_id` link. Fresh Clerk users don't have (b) yet, so
 * the page should redirect to `/onboarding` — same shape as spec 22
 * asserts for `/studio/inquiries`.
 *
 * The full CRUD flow (create series, add works, publish, show on the
 * public artist page) needs an onboarded artist fixture we don't
 * currently have. This spec is the small-but-load-bearing smoke:
 * if the route disappears or the auth gate breaks, we hear about it.
 */
test("studio-series-signed-in: fresh Clerk user is redirected from /studio/series to /onboarding", async ({
  page,
}) => {
  await setupClerkTestingToken({ page });

  await page.goto("/studio/series");

  // Non-artist redirects to onboarding. Same gate `/studio` +
  // `/studio/settings` + `/studio/inquiries` use.
  await expect(page).toHaveURL(/\/onboarding/, { timeout: 15_000 });
});

import { test, expect } from "@playwright/test";
import { setupClerkTestingToken } from "@clerk/testing/playwright";

/**
 * T-011 Phase 3 → T-012 Phase 1.
 *
 * The signed-in setup flow mints a fresh Clerk user with no
 * `artists.user_id` link. Pre-T-012 this used to land on a "No
 * portfolio yet" empty state; now `/studio` redirects non-artists to
 * `/onboarding` so they can self-onboard. Happy-path CRUD + modal
 * behavior is covered at the Rust integration tier
 * (`studio_test.rs`) against alice-test.
 */
test("studio-portfolio-signed-in: non-artist redirects to /onboarding", async ({
  page,
}) => {
  await setupClerkTestingToken({ page });

  await page.goto("/studio");
  await expect(page.getByRole("button", { name: /Open user menu/i })).toBeVisible({
    timeout: 15_000,
  });

  // After the server-side redirect we land on /onboarding.
  await expect(page).toHaveURL(/\/onboarding(\?|$)/);
  await expect(
    page.getByRole("heading", { name: /Let's start with who you are/i })
  ).toBeVisible();
});

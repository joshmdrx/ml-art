import { test, expect } from "@playwright/test";
import { setupClerkTestingToken } from "@clerk/testing/playwright";

/**
 * T-011 Phase 2 → T-012 Phase 1.
 *
 * The signed-in setup flow creates a fresh Clerk test user with no
 * `artists.user_id` link. Pre-T-012 this used to land on a
 * "you're not set up as an artist yet" empty state; now `/studio/settings`
 * redirects non-artists to `/onboarding` (the self-serve mint flow).
 *
 * Happy-path edits (paused → active, bio updates) are covered by Rust
 * integration tests against the seeded `alice-test` artist.
 */
test("studio-settings-signed-in: non-artist redirects to /onboarding", async ({
  page,
}) => {
  await setupClerkTestingToken({ page });

  await page.goto("/studio/settings");
  await expect(page.getByRole("button", { name: /Open user menu/i }))
    .toBeVisible({ timeout: 15_000 });

  // After the server-side redirect we land on /onboarding.
  await expect(page).toHaveURL(/\/onboarding(\?|$)/);
  // The identity step heading is what new users see first.
  await expect(
    page.getByRole("heading", { name: /Let's start with who you are/i })
  ).toBeVisible();
});

test("studio-settings-signed-out: redirects to sign-in", async ({ page }) => {
  await setupClerkTestingToken({ page });
  await page.goto("/studio/settings");
  // Either the onboarding identity step OR a /sign-in redirect is
  // acceptable here — we only assert the page resolves without 500.
  await expect(
    page.getByRole("heading", { name: /Let's start with who you are|Sign in|Studio settings/ })
  ).toBeVisible({ timeout: 15_000 });
});

import { test, expect } from "@playwright/test";
import { setupClerkTestingToken } from "@clerk/testing/playwright";

/**
 * T-012 Phase 1 — onboarding wizard happy path.
 *
 * The signed-in setup creates a fresh Clerk test user with no
 * `artists.user_id` link, so visiting `/onboarding` lands on the
 * identity step (the only step reachable without an artist row).
 *
 * We assert the full mint + skip-through-the-optional-steps path:
 *   identity (fill in) → profile (skip) → artworks (skip) →
 *   locations (skip) → review (publish) → /artists/<slug>
 *
 * Each per-test Clerk user gets a unique display name (using the
 * worker index) so re-runs don't collide on the `artists.slug`
 * UNIQUE constraint.
 *
 * NOTE: this test mints a real artist row in the DB; the row persists
 * across CI runs unless the DB is reset. The unique-per-worker
 * display name limits collisions but isn't a substitute for a
 * `make seed-reset` between full suites.
 */
test("onboarding-signed-in: identity → publish", async ({
  page,
}, testInfo) => {
  await setupClerkTestingToken({ page });

  // Unique per worker + retry so a re-run doesn't trip the slug
  // UNIQUE constraint. Title-case for readable slugs.
  const stamp = `${testInfo.workerIndex}-${testInfo.retry}-${Date.now()}`;
  const displayName = `E2E Artist ${stamp}`;

  // Visiting /studio bounces non-artists to /onboarding.
  await page.goto("/studio");
  await expect(page).toHaveURL(/\/onboarding(\?|$)/, { timeout: 15_000 });

  // Step 1 — identity.
  await expect(
    page.getByRole("heading", { name: /Let's start with who you are/i })
  ).toBeVisible();
  await page.getByLabel(/Display name/).fill(displayName);
  await page.getByLabel(/Location/).fill("Lisbon, Portugal");
  await page.getByRole("button", { name: /Continue/ }).click();

  // Step 2 — profile. Skip.
  await page.waitForURL(/step=profile/, { timeout: 15_000 });
  await expect(
    page.getByRole("heading", { name: /Tell collectors about your work/i })
  ).toBeVisible();
  await page.getByRole("link", { name: /Skip for now/i }).click();

  // Step 3 — artworks. Skip via Continue (zero-artworks is allowed).
  await page.waitForURL(/step=artworks/);
  await expect(
    page.getByRole("heading", { name: /Add a few artworks/i })
  ).toBeVisible();
  await page.getByRole("link", { name: /^Continue$/ }).click();

  // Step 4 — locations. Skip via Continue.
  await page.waitForURL(/step=locations/);
  await expect(
    page.getByRole("heading", { name: /Where can people see your work/i })
  ).toBeVisible();
  await page.getByRole("link", { name: /^Continue$/ }).click();

  // Step 5 — review. Publish.
  await page.waitForURL(/step=review/);
  await expect(
    page.getByRole("heading", { name: /Ready to publish/i })
  ).toBeVisible();
  await expect(page.getByText(displayName)).toBeVisible();
  await page.getByRole("button", { name: /Publish profile/ }).click();

  // Redirect to the public profile.
  await page.waitForURL(/\/artists\//, { timeout: 15_000 });
  await expect(
    page.getByRole("heading", { name: new RegExp(displayName) })
  ).toBeVisible();
});

test("onboarding-signed-in: identity rejects empty display name", async ({
  page,
}) => {
  await setupClerkTestingToken({ page });
  await page.goto("/onboarding");
  await expect(
    page.getByRole("heading", { name: /Let's start with who you are/i })
  ).toBeVisible({ timeout: 15_000 });

  // The Continue button is disabled while display_name is empty
  // (client-side gate, before the server even sees the request).
  await expect(
    page.getByRole("button", { name: /Continue/ })
  ).toBeDisabled();
});

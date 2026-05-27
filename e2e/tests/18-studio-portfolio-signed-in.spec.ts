import { test, expect } from "@playwright/test";
import { setupClerkTestingToken } from "@clerk/testing/playwright";

/**
 * T-011 Phase 3 — studio portfolio page.
 *
 * The Playwright signed-in setup creates a fresh Clerk user with no
 * `artists.user_id` link, so the realistic E2E target is the
 * not-an-artist empty state. Happy-path CRUD + modal behavior is
 * covered at the Rust integration tier (`studio_test.rs`) against
 * alice-test.
 */
test("studio-portfolio-signed-in: renders empty state for non-artist user", async ({
  page,
}) => {
  await setupClerkTestingToken({ page });

  await page.goto("/studio");
  await expect(page.getByRole("button", { name: /Open user menu/i })).toBeVisible({
    timeout: 15_000,
  });

  // Page title.
  await expect(page.getByRole("heading", { name: "Studio" })).toBeVisible();

  // Non-artist empty state. The grid + filter pills + modal must not render.
  await expect(
    page.getByRole("heading", { name: /No portfolio yet/i })
  ).toBeVisible();
  await expect(
    page.getByRole("toolbar", { name: "Filter by status" })
  ).toHaveCount(0);
  await expect(
    page.getByRole("button", { name: "+ New artwork" })
  ).toHaveCount(0);

  // Settings link still works.
  await expect(page.getByRole("link", { name: /Settings/ })).toBeVisible();
});

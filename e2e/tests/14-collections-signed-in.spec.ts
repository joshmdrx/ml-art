import { test, expect } from "@playwright/test";
import { setupClerkTestingToken } from "@clerk/testing/playwright";

/**
 * Signed-in `/collections` index renders without redirecting.
 *
 * Anonymous users get bounced to `/sign-in?redirect_url=/collections`
 * (covered by tests/10-save-signed-out.spec.ts via a different button).
 * Here we assert the inverse: a signed-in user lands on the page directly
 * and sees the heading.
 */
test("collections-signed-in: /collections renders for an authenticated user", async ({
  page,
}) => {
  await setupClerkTestingToken({ page });

  await page.goto("/collections");
  await expect(page).toHaveURL(/\/collections$/);
  await expect(
    page.getByRole("heading", { name: "Your collections" })
  ).toBeVisible();
});

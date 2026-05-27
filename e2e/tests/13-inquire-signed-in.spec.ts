import { test, expect } from "@playwright/test";
import { setupClerkTestingToken } from "@clerk/testing/playwright";

/**
 * Signed-in Inquire flow.
 *
 * The signed-in branch:
 *   - email is pre-filled from Clerk and read-only
 *   - no email verification step — the API marks `delivered_at = now()`
 *     immediately because the Clerk-verified email is trusted
 *   - modal shows "Sent. {Artist} will be in touch."
 */
test("inquire-signed-in: email pre-filled and immediate Sent state", async ({
  page,
}) => {
  await setupClerkTestingToken({ page });

  await page.goto("/search?q=ukiyo");
  await expect(page.getByRole("button", { name: /Open user menu/i }))
    .toBeVisible({ timeout: 15_000 });

  const firstTitle = page
    .locator("a[href^='/artworks/']:has-text('Untitled')")
    .first();
  await firstTitle.click();
  await expect(page).toHaveURL(/\/artworks\/[0-9a-f-]{36}/);

  await page.getByRole("button", { name: "Inquire" }).click();
  const dialog = page.getByRole("dialog");
  await expect(dialog).toBeVisible({ timeout: 10_000 });

  // Email field — should carry a value and be read-only.
  const emailInput = dialog.getByLabel("Email");
  await expect(emailInput).not.toBeEditable();
  await expect(emailInput).toHaveValue(/.+@.+\..+/);

  // Name *may* pre-fill from Clerk's `user.fullName`, but a test user
  // signed up with email only has no name set. Fill it explicitly so we
  // always exercise the submit path regardless of profile completeness.
  await dialog.getByLabel("Name").fill("E2E Tester");
  await dialog.getByLabel("Message").fill("Hi — interested in this piece.");
  await dialog.getByRole("button", { name: "Send inquiry" }).click();

  // Signed-in path: immediate "Sent" state, no "Check your inbox".
  await expect(dialog.getByText(/will be in touch/i)).toBeVisible({
    timeout: 10_000,
  });
  await expect(dialog.getByText(/Check your inbox/i)).toHaveCount(0);
});

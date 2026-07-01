import { test, expect } from "@playwright/test";
import { setupClerkTestingToken } from "@clerk/testing/playwright";

/**
 * T-083 — admin declines a pending artist via the confirm-dialog path.
 *
 * Uses `franz-pending` in seed — dedicated to this spec so it doesn't
 * race with spec 35 (which consumes `dora-pending`). Decline goes
 * through `useConfirm()` because it's destructive: the artist can't
 * be re-approved without going back to pending first.
 */
test("admin-artists-decline-admin-signed-in: pending row disappears after Decline confirm", async ({
  page,
}) => {
  await setupClerkTestingToken({ page });

  await page.goto("/admin/artists?status=pending");
  await expect(
    page.getByRole("heading", { name: /^Artists$/ }),
  ).toBeVisible({ timeout: 15_000 });

  const franz = page.getByText(/Franz Pending/);
  await expect(franz).toBeVisible({ timeout: 10_000 });

  // The Franz row's Decline button. Two pending rows exist (dora +
  // franz); we anchor by scoping the button to the row containing
  // the "Franz Pending" text.
  const franzRow = page
    .locator("div")
    .filter({ hasText: /Franz Pending/ })
    .filter({ has: page.getByRole("button", { name: /^Decline$/ }) })
    .first();
  await franzRow.getByRole("button", { name: /^Decline$/ }).click();

  // Radix AlertDialog confirm.
  const dialog = page.getByRole("alertdialog");
  await expect(dialog).toBeVisible({ timeout: 5_000 });
  await expect(dialog.getByText(/Decline Franz Pending/)).toBeVisible();
  await dialog.getByRole("button", { name: /^Decline$/ }).click();

  await expect(page.getByText(/Franz Pending: Declined/)).toBeVisible({
    timeout: 10_000,
  });
  await expect(franz).toHaveCount(0, { timeout: 10_000 });
});

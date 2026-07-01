import { test, expect } from "@playwright/test";
import { setupClerkTestingToken } from "@clerk/testing/playwright";

/**
 * T-083 — admin pause/unpause round-trip.
 *
 * Uses `greta-active` in seed — dedicated to this spec so pausing her
 * doesn't hide seeded artworks that other specs (spec 04 etc.) depend
 * on. Pause is destructive → useConfirm(); unpause is a straight click.
 */
test("admin-artists-pause-admin-signed-in: active → paused → active round-trip", async ({
  page,
}) => {
  await setupClerkTestingToken({ page });

  await page.goto("/admin/artists?status=active");
  await expect(
    page.getByRole("heading", { name: /^Artists$/ }),
  ).toBeVisible({ timeout: 15_000 });

  const greta = page.getByText(/Greta Active/);
  await expect(greta).toBeVisible({ timeout: 10_000 });

  // Pause opens a confirm AlertDialog. Scope to Greta's row so the
  // button target doesn't accidentally match alice/bruno/carmen.
  const gretaRow = page
    .locator("div")
    .filter({ hasText: /Greta Active/ })
    .filter({ has: page.getByRole("button", { name: /^Pause$/ }) })
    .first();
  await gretaRow.getByRole("button", { name: /^Pause$/ }).click();

  const dialog = page.getByRole("alertdialog");
  await expect(dialog).toBeVisible({ timeout: 5_000 });
  await dialog.getByRole("button", { name: /^Pause$/ }).click();

  await expect(page.getByText(/Greta Active: Paused/)).toBeVisible({
    timeout: 10_000,
  });
  await expect(greta).toHaveCount(0, { timeout: 10_000 });

  // Confirm she's in the paused tab, then unpause.
  await page.goto("/admin/artists?status=paused");
  const gretaPaused = page.getByText(/Greta Active/);
  await expect(gretaPaused).toBeVisible({ timeout: 10_000 });

  await page.getByRole("button", { name: /^Unpause$/ }).first().click();
  await expect(page.getByText(/Greta Active: Unpaused/)).toBeVisible({
    timeout: 10_000,
  });
  await expect(gretaPaused).toHaveCount(0, { timeout: 10_000 });
});

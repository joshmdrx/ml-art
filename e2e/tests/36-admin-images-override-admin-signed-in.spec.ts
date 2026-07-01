import { test, expect } from "@playwright/test";
import { setupClerkTestingToken } from "@clerk/testing/playwright";

/**
 * T-083.3 — admin overrides an auto-moderator rejection.
 *
 * Seed drops one rejected image on Carmen's Linocut Study
 * (`moderation_status='rejected'`, reason 'EXPLICIT_NUDITY'). The
 * admin queue at `/admin/images` lists it; clicking Override opens
 * a Radix AlertDialog confirm (T-071 pattern), and confirming fires
 * the server action + refresh.
 *
 * Same mutation-per-run caveat as spec 35 — CI is fresh, local
 * re-runs need a seed reset.
 */
test("admin-images-override-admin-signed-in: rejected image disappears after override", async ({
  page,
}) => {
  await setupClerkTestingToken({ page });

  await page.goto("/admin/images");
  await expect(
    page.getByRole("heading", { name: /^Image moderation$/i }),
  ).toBeVisible({ timeout: 15_000 });

  // The rejected row has the reason chip visible on top of the
  // 50%-opacity thumbnail; the artist name is the stable anchor.
  const carmen = page.getByRole("link", { name: /Carmen Test/i }).first();
  await expect(carmen).toBeVisible({ timeout: 10_000 });

  // Row-level button ("Override (approve)") opens the confirm dialog.
  await page.getByRole("button", { name: /^Override .approve.$/ }).click();

  // Confirm inside the AlertDialog (Radix). Scope to the dialog to
  // avoid matching the still-rendered row button underneath.
  const dialog = page.getByRole("alertdialog");
  await expect(dialog).toBeVisible({ timeout: 5_000 });
  await dialog.getByRole("button", { name: /^Override .approve.$/ }).click();

  // Toast surface — `Approved: <artwork title or s3_key>`. Seed's
  // rejected image is on artwork "Linocut Study".
  await expect(page.getByText(/Approved: Linocut Study/)).toBeVisible({
    timeout: 10_000,
  });

  // Row is out of the rejected queue.
  await expect(carmen).toHaveCount(0, { timeout: 10_000 });
});

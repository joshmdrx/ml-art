import { readFileSync } from "node:fs";
import { test, expect } from "@playwright/test";
import { setupClerkTestingToken } from "@clerk/testing/playwright";
import { createArtwork } from "../lib/fixtures";

/**
 * URL-driven ArtworkEditModal lifecycle (T-071 primitive + docs/ui-patterns.md
 * → "Multi-step modals").
 *
 * StudioPortfolio drives the modal off `?id=`: uuid opens edit, "new"
 * opens create. Close removes the param. This spec seeds an artwork
 * via the test-fixtures seam, then asserts:
 *   - direct navigation to `/studio?id=<uuid>` opens the modal with
 *     the artwork's title pre-filled
 *   - Escape closes the modal AND strips the param from the URL
 */
test("studio-artwork-modal-artist-signed-in: ?id=<uuid> opens edit modal, Escape strips the param", async ({
  page,
}) => {
  await setupClerkTestingToken({ page });

  const meta = JSON.parse(
    readFileSync("e2e/.auth/artist-meta.json", "utf8"),
  ) as { slug: string };

  const title = `E2E modal test ${Date.now().toString(36)}`;
  const artwork = await createArtwork({
    artistSlug: meta.slug,
    title,
    status: "draft",
    withImage: true,
  });

  await page.goto(`/studio?id=${artwork.id}`);
  const dialog = page.getByRole("dialog");
  await expect(dialog).toBeVisible({ timeout: 15_000 });

  // Title input is pre-filled with the artwork's title.
  const titleInput = dialog.getByLabel(/Title/);
  await expect(titleInput).toBeVisible({ timeout: 10_000 });
  await expect(titleInput).toHaveValue(title);

  // Escape → Radix onOpenChange(false) → parent clears ?id.
  await page.keyboard.press("Escape");
  await expect(dialog).not.toBeVisible({ timeout: 5_000 });
  await expect(page).toHaveURL(/\/studio(\?|$)/);
  expect(page.url()).not.toMatch(/[?&]id=/);
});

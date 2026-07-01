import { readFileSync } from "node:fs";
import { test, expect } from "@playwright/test";
import { setupClerkTestingToken } from "@clerk/testing/playwright";
import { createArtwork } from "../lib/fixtures";

/**
 * T-070 + T-073 — publish nudge on draft → published transition
 * when the artwork is missing dimensions and/or medium_category.
 *
 * Fires from ArtworkEditModal's Save handler. Uses the T-071
 * `useConfirm()` primitive (Radix AlertDialog). Non-blocking — the
 * artist can still publish; the dialog just flags what buyers won't
 * be able to filter by.
 *
 * Fixture: draft artwork with no dimensions + no medium_category
 * (both NULL in the test-fixtures insert). Both conditions active
 * → nudge title reads "Publish without dimensions or a medium
 * category?".
 */
test("publish-nudge-artist-signed-in: draft → published without dimensions/category triggers the confirm nudge", async ({
  page,
}) => {
  await setupClerkTestingToken({ page });

  const meta = JSON.parse(
    readFileSync("e2e/.auth/artist-meta.json", "utf8"),
  ) as { slug: string };

  const title = `E2E nudge test ${Date.now().toString(36)}`;
  const artwork = await createArtwork({
    artistSlug: meta.slug,
    title,
    status: "draft",
    withImage: true,
    // dimensions omitted → NULL → triggers nudge
    // medium omitted → medium_category NULL → triggers nudge
  });

  await page.goto(`/studio?id=${artwork.id}`);
  const dialog = page.getByRole("dialog");
  await expect(dialog).toBeVisible({ timeout: 15_000 });
  await expect(dialog.getByLabel(/Title/)).toHaveValue(title);

  // Flip status → published + Save. The Save handler sees a
  // draft→published transition and both fields NULL → nudge fires.
  await dialog.getByLabel(/^Status$/).selectOption("published");
  await dialog.getByRole("button", { name: /^Save$|^Save changes$/ }).click();

  const nudge = page.getByRole("alertdialog");
  await expect(nudge).toBeVisible({ timeout: 10_000 });
  await expect(nudge).toContainText(/Publish without/i);
  // Both missing conditions surface in the copy — either "dimensions
  // or a medium category" or "a medium category or dimensions"
  // depending on iteration order of the `missing` array.
  await expect(nudge).toContainText(/dimensions/i);
  await expect(nudge).toContainText(/medium category/i);

  // Cancel keeps the artwork as draft — we don't want to actually
  // publish + reshape downstream specs' visible-artwork counts.
  await nudge
    .getByRole("button", { name: /Keep editing|Cancel/ })
    .click();
  await expect(nudge).not.toBeVisible({ timeout: 5_000 });
});

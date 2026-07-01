import { test, expect } from "@playwright/test";

/**
 * Artwork image viewer — lightbox open/close.
 *
 * The main image on `/artworks/[id]` is wrapped in a button with
 * `aria-label="Open image in full view"`; clicking opens a Radix
 * Dialog holding the same image at 95vw/95vh. Escape or the ×
 * closes it.
 *
 * This is the visible-to-every-user UX added in the 2026-07-01 studio
 * polish batch. If a regression re-hides the main-image click target
 * or drops the Radix Portal, this catches it.
 *
 * Multi-image thumbnail behavior (`role="tab"` swap) needs a seeded
 * artwork with >1 image — deferred until the seed carries multi-image
 * works.
 */
test("artwork-image-viewer: clicking main image opens lightbox; Escape closes", async ({
  page,
}) => {
  await page.goto("/search?q=ukiyo");
  const artworkLink = page
    .locator("a[href^='/artworks/']:has-text('Untitled')")
    .first();
  await artworkLink.click();
  await expect(page).toHaveURL(/\/artworks\/[0-9a-f-]{36}/);

  const opener = page.getByRole("button", { name: "Open image in full view" });
  await expect(opener).toBeVisible({ timeout: 10_000 });

  await opener.click();

  // Radix renders the dialog in a portal at the document root; the
  // Close button carries the identifying aria-label.
  const closeBtn = page.getByRole("button", { name: "Close full view" });
  await expect(closeBtn).toBeVisible({ timeout: 5_000 });

  await page.keyboard.press("Escape");
  await expect(closeBtn).not.toBeVisible({ timeout: 5_000 });
});

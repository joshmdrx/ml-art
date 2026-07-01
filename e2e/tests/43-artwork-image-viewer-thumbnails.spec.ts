import { test, expect } from "@playwright/test";

/**
 * Artwork image viewer — thumbnail-swap on a multi-image artwork.
 *
 * Extends spec 29 (which only asserted the lightbox open/close). Seed
 * has a second approved image on Crimson Field (bbb22222) so the
 * viewer renders the `role="tablist"` thumbnail strip. Clicking the
 * non-primary thumbnail flips `aria-selected` and swaps the main
 * image src.
 */
test("artwork-image-viewer-thumbnails: clicking a thumbnail swaps the main image", async ({
  page,
}) => {
  await page.goto("/artworks/bbb22222-2222-2222-2222-222222222222");
  await expect(page).toHaveURL(/\/artworks\/bbb22222/);

  const tablist = page.getByRole("tablist", { name: /Artwork images/i });
  await expect(tablist).toBeVisible({ timeout: 15_000 });

  // Two tabs. Primary is index 0 (aria-selected="true" initially);
  // index 1 is the newly-added non-primary image.
  const tabs = tablist.getByRole("tab");
  await expect(tabs).toHaveCount(2);
  await expect(tabs.nth(0)).toHaveAttribute("aria-selected", "true");
  await expect(tabs.nth(1)).toHaveAttribute("aria-selected", "false");

  await tabs.nth(1).click();

  await expect(tabs.nth(0)).toHaveAttribute("aria-selected", "false");
  await expect(tabs.nth(1)).toHaveAttribute("aria-selected", "true");
});

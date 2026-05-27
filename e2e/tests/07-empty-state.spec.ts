import { test, expect } from "@playwright/test";

/**
 * Free-text queries can't reliably hit an empty result set when vector
 * search is on — CLIP returns *something* even for nonsense strings. The
 * structured-filter path is deterministic: a location that no seeded
 * studio uses returns zero rows.
 */
test("a search with an impossible filter renders the empty state", async ({
  page,
}) => {
  await page.goto("/search?location=nowhere-no-studio-here");

  await expect(
    page.getByText(/No artworks match this search/i)
  ).toBeVisible();
});

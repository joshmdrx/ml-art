import { test, expect } from "@playwright/test";

/**
 * T-058.3 — public series detail page.
 *
 * Seed drops one series under alice-test: `blue-period` with two
 * artworks (Blue Morning + Crimson Field). The detail page renders
 * a header + statement + grid of member artworks.
 *
 * Anonymous: no auth required.
 */
test("series-detail: /artists/[slug]/series/[seriesSlug] renders header + member artworks", async ({
  page,
}) => {
  await page.goto("/artists/alice-test/series/blue-period");
  await expect(page).toHaveURL(/\/artists\/alice-test\/series\/blue-period/);

  await expect(
    page.getByRole("heading", { name: /^Blue Period$/ }),
  ).toBeVisible({ timeout: 15_000 });

  // Member artworks — at least one is in the grid.
  await expect(
    page.locator("a[href^='/artworks/']").first(),
  ).toBeVisible({ timeout: 10_000 });
});

test("series-detail: unknown series slug 404s", async ({ page }) => {
  const resp = await page.goto("/artists/alice-test/series/does-not-exist");
  expect(resp?.status()).toBe(404);
});

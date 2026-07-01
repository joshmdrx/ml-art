import { test, expect } from "@playwright/test";

/**
 * T-053 — public collection page + OG card.
 *
 * Seed drops a public collection with a fixed share_id
 * (`test-share-alice`, owned by the generic user_test_99). The public
 * page renders anonymously; Next auto-injects an `og:image` meta tag
 * pointing at the file-convention route at `/c/<share_id>/opengraph-image`.
 *
 * Mirrors spec 30's shape — meta URL + GET returns image/*.
 */
test("public-collection-og: /c/test-share-alice renders name + emits an og:image that resolves", async ({
  page,
}) => {
  await page.goto("/c/test-share-alice");
  await expect(
    page.getByRole("heading", { name: /^Public Test Board$/ }),
  ).toBeVisible({ timeout: 15_000 });

  const metaUrl = await page
    .locator('meta[property="og:image"]')
    .first()
    .getAttribute("content");
  expect(metaUrl).toBeTruthy();
  expect(metaUrl!).toMatch(/opengraph-image/);

  const resp = await page.request.get(metaUrl!, { timeout: 30_000 });
  expect(resp.status()).toBe(200);
  expect(resp.headers()["content-type"] ?? "").toMatch(/^image\//);
});

test("public-collection-og: unknown share_id 404s", async ({ page }) => {
  const resp = await page.goto("/c/does-not-exist");
  expect(resp?.status()).toBe(404);
});

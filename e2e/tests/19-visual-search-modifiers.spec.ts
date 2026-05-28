import { test, expect } from "@playwright/test";

/**
 * T-010 Phase D — visual search UI smoke.
 *
 * What this covers (no real upload — that requires the
 * Jina-can't-reach-localhost workaround from Phase A's dev limitation):
 *   - The "Search by image" affordance renders on the homepage hero
 *   - Visiting /search with a synthetic `image_upload_id` query param
 *     shows the modifier bar (because the server fetched
 *     /v1/modifiers) and the visual-anchor strip
 *
 * The server-side rendered ModifierBar is what we assert on. The full
 * upload → ranked-results round trip needs a tunneled MinIO or staging
 * env to test end-to-end.
 */

test("homepage shows the visual-search affordance", async ({ page }) => {
  await page.goto("/");
  await expect(
    page.getByRole("button", { name: /Search by image/ })
  ).toBeVisible({ timeout: 10_000 });
});

test("/search with image_upload_id renders modifier bar + anchor strip", async ({
  page,
}) => {
  // Synthetic UUID — the row doesn't exist, so the search API will
  // 404 on the actual results query, but the page still renders the
  // shell (search summary, anchor strip, modifier bar from
  // /v1/modifiers, error state for the missing results). Good enough
  // to assert the UI assembled correctly.
  const fakeId = "00000000-0000-7000-8000-000000000000";
  await page.goto(`/search?image_upload_id=${fakeId}`);

  // Anchor strip with the truncated id + clear link.
  await expect(page.getByText(/Searching by image/i)).toBeVisible({
    timeout: 10_000,
  });
  await expect(page.getByRole("link", { name: /Clear image/i })).toBeVisible();

  // Modifier bar — five buttons from the static registry.
  await expect(
    page.getByRole("toolbar", { name: "Visual-search modifiers" })
  ).toBeVisible();
  for (const label of ["Moodier", "Warmer", "More minimal"]) {
    await expect(
      page.getByRole("button", { name: label, exact: true })
    ).toBeVisible();
  }
});

test("clicking a modifier toggles the URL parameter", async ({ page }) => {
  const fakeId = "00000000-0000-7000-8000-000000000000";
  await page.goto(`/search?image_upload_id=${fakeId}`);
  await expect(
    page.getByRole("button", { name: "Moodier", exact: true })
  ).toBeVisible({ timeout: 10_000 });

  await page.getByRole("button", { name: "Moodier", exact: true }).click();
  await page.waitForURL(/modifiers=moodier/);
  expect(page.url()).toMatch(/modifiers=moodier/);

  // Pressed state via aria-pressed.
  await expect(
    page.getByRole("button", { name: "Moodier", exact: true })
  ).toHaveAttribute("aria-pressed", "true");
});

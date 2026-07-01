import { test, expect } from "@playwright/test";
import { setupClerkTestingToken } from "@clerk/testing/playwright";

/**
 * T-084.2 — admin bypass on non-active artist pages.
 *
 * `/artists/[slug]` normally 404s for non-active artists (spec 39
 * covers that). Admins get through with an `AdminArtistBanner`
 * pinned at the top of the page. Seed drops `edith-paused` with
 * status='paused' for this test.
 */
test("admin-artist-banner-admin-signed-in: admin sees paused-artist banner + page renders", async ({
  page,
}) => {
  await setupClerkTestingToken({ page });

  await page.goto("/artists/edith-paused");
  await expect(page).toHaveURL(/\/artists\/edith-paused/);

  // Admin banner is present with the paused status word.
  await expect(page.getByText(/^Admin view$/)).toBeVisible({ timeout: 15_000 });
  await expect(
    page.getByText(/This artist is/).filter({ hasText: /Paused/ }),
  ).toBeVisible();

  // Unpause action is exposed inline on paused-status banner.
  await expect(page.getByRole("button", { name: /^Unpause$/ })).toBeVisible();

  // The real artist page below the banner also renders — headline is
  // the display_name from seed.
  await expect(
    page.getByRole("heading", { name: /^Edith Paused$/ }),
  ).toBeVisible();
});

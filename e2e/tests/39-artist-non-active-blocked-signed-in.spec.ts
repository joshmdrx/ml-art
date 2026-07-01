import { test, expect } from "@playwright/test";
import { setupClerkTestingToken } from "@clerk/testing/playwright";

/**
 * T-084.2 — non-admin cannot view a non-active artist page.
 *
 * Complement to spec 38: seed's `edith-paused` (status='paused') is
 * hidden from the public site. A regular signed-in user hitting
 * `/artists/edith-paused` should get the Next 404, not the artist
 * page (which is admin-only for non-active statuses).
 *
 * Runs under `chromium-authed` (regular user, is_admin=false).
 */
test("artist-non-active-blocked-signed-in: non-admin visitor gets a 404 for a paused artist", async ({
  page,
}) => {
  await setupClerkTestingToken({ page });

  const resp = await page.goto("/artists/edith-paused");
  expect(resp?.status()).toBe(404);

  // The banner MUST NOT render for non-admins even if the page shell
  // somehow loaded.
  await expect(page.getByText(/^Admin view$/)).toHaveCount(0);
  await expect(
    page.getByRole("heading", { name: /^Edith Paused$/ }),
  ).toHaveCount(0);
});

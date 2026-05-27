import { test, expect } from "@playwright/test";

/**
 * Signed-out users clicking "Save to collection" should bounce to the
 * sign-in page with a `redirect_url` back to the artwork they were on.
 *
 * The signed-in save flow (modal, toggle, inline create) is covered in
 * `12-save-signed-in.spec.ts` under the `chromium-authed` project.
 */
test("signed-out save button redirects to /sign-in with redirect_url", async ({
  page,
}) => {
  await page.goto("/search?q=ukiyo");

  const firstTitle = page
    .locator("a[href^='/artworks/']:has-text('Untitled')")
    .first();
  await firstTitle.click();
  await expect(page).toHaveURL(/\/artworks\/[0-9a-f-]{36}/);

  // Click Save → triggers a client-side router.push to /sign-in?...
  await page.getByRole("button", { name: "Save to collection" }).click();

  // We end up on Clerk's hosted sign-in (rendered at /sign-in/...) with a
  // redirect_url param pointing back to the artwork detail.
  await page.waitForURL(/\/sign-in/);
  const url = new URL(page.url());
  expect(url.searchParams.get("redirect_url")).toMatch(/^\/artworks\//);
});

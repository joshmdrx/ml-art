import { test, expect } from "@playwright/test";

test("location filter narrows results to the named city", async ({ page }) => {
  await page.goto("/search?location=berlin");

  // The query summary echoes the location.
  await expect(page.getByText(/in berlin/i)).toBeVisible();

  // The seed puts a few studios in Berlin (deterministic hash). We just
  // need to assert that at least one artwork image rendered — the
  // structured-filter assertion is on the API; here we're confirming the
  // page didn't bail to an empty state.
  await expect(page.locator("img[src*='/artworks/']").first()).toBeVisible();
});

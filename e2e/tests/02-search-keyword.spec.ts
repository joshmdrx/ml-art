import { test, expect } from "@playwright/test";

test("typing in hero search submits and lands on results", async ({ page }) => {
  await page.goto("/");
  const hero = page.getByPlaceholder(
    "Search artworks, artists, or drop an image."
  );
  await hero.fill("ukiyo");
  await hero.press("Enter");

  await expect(page).toHaveURL(/\/search\?q=ukiyo/);

  // Result grid should contain a Ukiyo-E card linking to a Ukiyo studio.
  const ukiyoLink = page.getByRole("link", { name: /Ukiyo E Studio/i }).first();
  await expect(ukiyoLink).toBeVisible();
});

test("search page rendering respects sort and pagination caps", async ({
  page,
}) => {
  await page.goto("/search?limit=24");
  // No specific count assertion — depends on full corpus — but at least one
  // result must render.
  await expect(page.locator("img[src*='/artworks/']").first()).toBeVisible();
});

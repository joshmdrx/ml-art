import { test, expect } from "@playwright/test";

test("browsing from /neighborhoods into a detail page", async ({ page }) => {
  await page.goto("/neighborhoods");

  await expect(
    page.getByRole("heading", { name: "Neighborhoods" })
  ).toBeVisible();

  const card = page
    .getByRole("link", { name: /Fields of Color/i })
    .first();
  await expect(card).toBeVisible();
  await card.click();

  await expect(page).toHaveURL(/\/neighborhoods\/fields-of-color/);
  await expect(
    page.getByRole("heading", { name: /Fields of Color/i })
  ).toBeVisible();
  // Detail page shows the artworks grid.
  await expect(page.locator("img[src*='/artworks/']").first()).toBeVisible();
});

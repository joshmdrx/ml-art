import { test, expect } from "@playwright/test";

test("clicking an artist name on a card opens that artist's portfolio", async ({
  page,
}) => {
  // Use the keyword-medium match (deterministic regardless of vector ranking).
  await page.goto("/search?q=ukiyo");

  // Click the specific Ukiyo E Studio link rather than .first() — vector
  // search may shuffle ordering across runs, so picking the named anchor
  // is more reliable than the first artist link.
  const link = page
    .locator("a[href='/artists/demo-ukiyo-e']")
    .first();
  await expect(link).toBeVisible();
  await link.click();

  await expect(page).toHaveURL(/\/artists\/demo-ukiyo-e/);
  await expect(
    page.getByRole("heading", { name: /Ukiyo E Studio/i })
  ).toBeVisible();
  await expect(page.locator("img[src*='/artworks/']").first()).toBeVisible();
});

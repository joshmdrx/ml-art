import { test, expect } from "@playwright/test";

test("clicking an artwork from search opens the detail page", async ({
  page,
}) => {
  await page.goto("/search?q=ukiyo");

  // The first card's title link goes to /artworks/<uuid>.
  const titleLink = page
    .locator("a[href^='/artworks/']:has-text('Untitled')")
    .first();
  await titleLink.click();

  await expect(page).toHaveURL(/\/artworks\/[0-9a-f-]{36}/);

  // The detail page shows artist link, "More like this" row, and an image.
  await expect(page.getByText(/^by /).first()).toBeVisible();
  await expect(page.getByRole("heading", { name: "More like this" })).toBeVisible();
  await expect(page.locator("img").first()).toBeVisible();
});

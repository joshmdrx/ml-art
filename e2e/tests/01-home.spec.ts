import { test, expect } from "@playwright/test";

test("homepage loads with hero search, neighborhoods, and recent grid", async ({
  page,
}) => {
  await page.goto("/");

  // Hero search input is present and focusable
  const hero = page.getByPlaceholder(
    "Search artworks, artists, or drop an image."
  );
  await expect(hero).toBeVisible();

  // "Recently added" section renders with at least one artwork image
  await expect(page.getByRole("heading", { name: "Recently added" })).toBeVisible();
  const recentImages = page.locator("section >> img[src*='/artworks/']");
  await expect(recentImages.first()).toBeVisible();

  // Neighborhoods section: at least one named neighborhood link
  await expect(
    page.getByRole("heading", { name: "Explore neighborhoods" })
  ).toBeVisible();
  await expect(
    page.getByRole("link", { name: /The Impressionists/ })
  ).toBeVisible();
});

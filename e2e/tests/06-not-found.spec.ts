import { test, expect } from "@playwright/test";

test("unknown artist slug renders 404", async ({ page }) => {
  const response = await page.goto("/artists/this-artist-does-not-exist");
  expect(response?.status()).toBe(404);
});

test("unknown artwork id renders 404", async ({ page }) => {
  const response = await page.goto(
    "/artworks/00000000-0000-0000-0000-000000000000"
  );
  expect(response?.status()).toBe(404);
});

test("unknown neighborhood slug renders 404", async ({ page }) => {
  const response = await page.goto("/neighborhoods/not-a-real-neighborhood");
  expect(response?.status()).toBe(404);
});

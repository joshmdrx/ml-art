import { test, expect } from "@playwright/test";

/**
 * T-038 G5 — `/search?map=1` map mode smoke.
 *
 * The seeded WikiArt artists have no `artist_locations` rows yet
 * (real geocoding is a separate seed step, deferred for the demo
 * corpus). So this spec covers the shell + toggle behavior, not the
 * pin-clicking end-to-end. The full path is exercised by:
 *   - Rust integration tests in `tests/search_map_test.rs` (response
 *     shape + filter behavior against a seeded fixture)
 *   - Manual smoke against a real Mapbox token + seeded location row
 *
 * What this spec asserts:
 *   - The Grid / Map view toggle renders on `/search`
 *   - Clicking "Map" navigates to `?map=1` and the toggle reflects
 *     the new state
 *   - Filters in the URL are preserved across the toggle
 *   - The map region (or fallback) is present without runtime errors
 */

test("search page renders the Grid / Map view toggle", async ({ page }) => {
  await page.goto("/search");
  await expect(
    page.getByRole("tab", { name: "Grid", exact: true })
  ).toBeVisible({ timeout: 10_000 });
  await expect(page.getByRole("tab", { name: "Map", exact: true })).toBeVisible();
  // Grid is the default selection.
  await expect(
    page.getByRole("tab", { name: "Grid", exact: true })
  ).toHaveAttribute("aria-selected", "true");
});

test("Map toggle navigates to ?map=1 and reflects selection", async ({
  page,
}) => {
  await page.goto("/search");
  await page.getByRole("tab", { name: "Map", exact: true }).click();
  await page.waitForURL(/\?map=1/);

  await expect(
    page.getByRole("tab", { name: "Map", exact: true })
  ).toHaveAttribute("aria-selected", "true");
  await expect(
    page.getByRole("tab", { name: "Grid", exact: true })
  ).toHaveAttribute("aria-selected", "false");

  // The page either renders the Mapbox region (token set) or the
  // fallback grid of pin cards (token absent). With no seeded
  // locations in the demo corpus the fallback shows "No venues match
  // this search."
  const mapRegion = page.getByRole("region", {
    name: /Map of locations matching/i,
  });
  const fallbackEmpty = page.getByText(/No venues match this search/i);
  await expect(mapRegion.or(fallbackEmpty)).toBeVisible({ timeout: 10_000 });
});

test("toggling between Grid and Map preserves the query filter", async ({
  page,
}) => {
  await page.goto("/search?q=ukiyo");
  await page.getByRole("tab", { name: "Map", exact: true }).click();
  await page.waitForURL(/q=ukiyo.*map=1|map=1.*q=ukiyo/);

  await page.getByRole("tab", { name: "Grid", exact: true }).click();
  await page.waitForURL(/q=ukiyo/);
  // `map=1` must be gone (we drop it when leaving map mode).
  expect(page.url()).not.toMatch(/[?&]map=/);
  // `bbox` likewise — the toggle strips it so a stale bbox doesn't
  // poison the next grid query.
  expect(page.url()).not.toMatch(/bbox=/);
});

test("malformed bbox returns 400 from the map API (status banner)", async ({
  request,
}) => {
  // Direct API hit — easier to assert than the UI's error banner.
  // The Playwright test runner has `request` baked in, which uses
  // the same baseURL as the page.
  const res = await request.get(
    `${process.env.NEXT_PUBLIC_API_BASE_URL ?? "http://localhost:9100"}/v1/search/map?bbox=not,a,bbox`
  );
  expect(res.status()).toBe(400);
});

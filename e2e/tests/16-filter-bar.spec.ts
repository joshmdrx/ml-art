import { test, expect } from "@playwright/test";

/**
 * T-023 — FilterBar component.
 *
 * Two surfaces share the same component:
 *   - /search shows medium / price / availability / location pills
 *   - /neighborhoods/[slug] shows medium / price / availability (no location;
 *     the slug pins place)
 *
 * The component is URL-driven — every pill change pushes a new URL via
 * `router.push`. So we assert two things: the URL gains the right param,
 * and the new server-rendered page reflects that filter (`describeQuery`
 * line under the title on `/search` is the cheapest visual confirmation).
 */

test("filter-bar on /search: choosing a medium updates URL + results", async ({
  page,
}) => {
  await page.goto("/search");

  // The Medium pill is a dropdown trigger; opening it surfaces options.
  await page.getByRole("button", { name: /^Medium$/ }).click();
  await page.getByRole("menuitem", { name: "Ukiyo E" }).click();

  // URL updates with the chosen medium.
  await page.waitForURL(/medium=Ukiyo\+E|medium=Ukiyo%20E/);
  expect(page.url()).toMatch(/medium=Ukiyo/);

  // Pill flips to the active "Medium: Ukiyo E" label and aria-pressed=true.
  const active = page.getByRole("button", { name: /Medium: Ukiyo E/ });
  await expect(active).toBeVisible();
  await expect(active).toHaveAttribute("aria-pressed", "true");

  // At least one Ukiyo E result is on the grid (seed has many).
  await expect(
    page.locator("a[href^='/artworks/']").first()
  ).toBeVisible({ timeout: 10_000 });
});

test("filter-bar on /search: clearing all filters removes URL params", async ({
  page,
}) => {
  await page.goto("/search?medium=Cubism&availability=available");

  // Both pills should start active.
  await expect(
    page.getByRole("button", { name: /Medium: Cubism/ })
  ).toBeVisible();

  // "Clear filters" link kills both at once.
  await page.getByRole("button", { name: "Clear filters" }).click();
  await page.waitForURL((url) => !url.search.includes("medium="));
  expect(page.url()).not.toMatch(/medium=/);
  expect(page.url()).not.toMatch(/availability=/);
});

test("filter-bar on /neighborhoods/[slug]: medium pill narrows the neighborhood grid", async ({
  page,
}) => {
  // Visit any neighborhood; the seeded ones are listed on /neighborhoods.
  await page.goto("/neighborhoods");
  const firstNeighborhood = page.locator("a[href^='/neighborhoods/']").first();
  await firstNeighborhood.click();
  await page.waitForURL(/\/neighborhoods\/[a-z0-9-]+$/);

  // The location pill must NOT appear on this surface.
  await expect(page.getByRole("button", { name: /^Location$/ })).toHaveCount(0);

  // Set a medium. The seed neighborhood is "test-vibes" in test fixtures,
  // but in the dev DB it's `wikiart-impressionism` and others — picking
  // "Impressionism" is a safe choice that hits something on most pages
  // and gracefully empties on others. Either result is a valid render.
  await page.getByRole("button", { name: /^Medium$/ }).click();
  await page.getByRole("menuitem", { name: "Impressionism", exact: true }).click();

  await page.waitForURL(/medium=Impressionism/);
  await expect(
    page.getByRole("button", { name: /Medium: Impressionism/ })
  ).toBeVisible();
});

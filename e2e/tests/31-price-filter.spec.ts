import { test, expect } from "@playwright/test";

/**
 * T-080 — currency-aware price filter (canonical GBP).
 *
 * FilterBar's Price pill exposes four bucket presets keyed by GBP
 * (pence stored, £ formatted). Clicking a bucket pushes `price=<token>`
 * onto the URL and re-server-renders the results grid. This spec
 * covers the discrete-bucket path — the free-form min/max range is
 * covered by the FilterBar unit tests + spec 16's generic assertions.
 *
 * Chosen bucket: "Under £500" (`price=u500`). Seed has a spread of
 * demo prices, so at least some but not all artworks fall inside.
 * We assert the URL param + active-pill state; the grid may or may
 * not have results but the empty-state fallback is covered elsewhere.
 */
test("price-filter: choosing 'Under £500' updates URL + active pill", async ({
  page,
}) => {
  await page.goto("/search");

  await page.getByRole("button", { name: /^Price$/ }).click();
  await page.getByRole("menuitem", { name: "Under £500" }).click();

  await page.waitForURL(/price=u500/);
  expect(page.url()).toMatch(/price=u500/);

  const active = page.getByRole("button", { name: /Price: Under £500/ });
  await expect(active).toBeVisible();
  await expect(active).toHaveAttribute("aria-pressed", "true");
});

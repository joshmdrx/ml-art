import { test, expect } from "@playwright/test";
import { setupClerkTestingToken } from "@clerk/testing/playwright";

/**
 * Studio sidebar navigation smoke — the 2026-07-01 UX rewrite.
 *
 * Runs under `chromium-artist` (onboarded artist fixture) — a non-
 * artist visiting `/studio` bounces to `/onboarding` and never sees
 * the sidebar. We assert:
 *   - All 4 nav items render (Portfolio, Series, Inquiries, Settings)
 *   - The `aria-current="page"` badge lands on the right item as we
 *     navigate between subpages
 *
 * If the sidebar drops a nav item or the active-state logic breaks
 * (exact vs prefix match), this catches it.
 */
test("studio-sidebar-artist-signed-in: all 4 nav items render + active state follows the URL", async ({
  page,
}) => {
  await setupClerkTestingToken({ page });

  await page.goto("/studio");
  await expect(page).toHaveURL(/\/studio(\?|$)/, { timeout: 15_000 });

  const nav = page.getByRole("navigation", { name: /Studio navigation/i });
  await expect(nav).toBeVisible({ timeout: 15_000 });

  // Portfolio is active on /studio (exact-match rule).
  const portfolio = nav.getByRole("link", { name: /^Portfolio$/ });
  const series = nav.getByRole("link", { name: /^Series$/ });
  const inquiries = nav.getByRole("link", { name: /^Inquiries/ });
  const settings = nav.getByRole("link", { name: /^Settings$/ });

  for (const link of [portfolio, series, inquiries, settings]) {
    await expect(link).toBeVisible();
  }
  await expect(portfolio).toHaveAttribute("aria-current", "page");

  // Navigate to Series — active flips.
  await series.click();
  await expect(page).toHaveURL(/\/studio\/series/);
  await expect(nav.getByRole("link", { name: /^Series$/ })).toHaveAttribute(
    "aria-current",
    "page",
  );
  await expect(
    nav.getByRole("link", { name: /^Portfolio$/ }),
  ).not.toHaveAttribute("aria-current", "page");

  // Navigate to Settings.
  await nav.getByRole("link", { name: /^Settings$/ }).click();
  await expect(page).toHaveURL(/\/studio\/settings/);
  await expect(nav.getByRole("link", { name: /^Settings$/ })).toHaveAttribute(
    "aria-current",
    "page",
  );
});

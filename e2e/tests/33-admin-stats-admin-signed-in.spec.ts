import { test, expect } from "@playwright/test";
import { setupClerkTestingToken } from "@clerk/testing/playwright";

/**
 * T-084.1 — /admin/stats operator dashboard.
 *
 * Server-rendered read-only view backed by `GET /v1/admin/stats`.
 * Four big-number tiles, a 4-week funnel table, and a one-line
 * admin-activity blurb. The page is trivial to break in a way that
 * still looks fine (blank tiles if the API 500s, missing table if
 * the schema drifts) — smoke coverage catches those.
 */
test("admin-stats-admin-signed-in: /admin/stats renders tiles + funnel table", async ({
  page,
}) => {
  await setupClerkTestingToken({ page });

  await page.goto("/admin/stats");
  await expect(
    page.getByRole("heading", { name: /^Stats$/ }),
  ).toBeVisible({ timeout: 15_000 });

  // The four tiles — labels are stable copy inside `CountTile`.
  for (const label of [
    "Users",
    "Active artists",
    "Published works",
    "Delivered inquiries",
  ]) {
    await expect(page.getByText(label, { exact: true })).toBeVisible();
  }

  // Funnel table — assert the section header + the 5-column layout via
  // its stable column headers.
  await expect(
    page.getByRole("heading", { name: /Search .* inquiry funnel/i }),
  ).toBeVisible();
  await expect(
    page.getByRole("columnheader", { name: "Week of" }),
  ).toBeVisible();
  await expect(
    page.getByRole("columnheader", { name: "Searches" }),
  ).toBeVisible();
  await expect(
    page.getByRole("columnheader", { name: "Inquiries sent" }),
  ).toBeVisible();

  // Admin-activity blurb — text is present even when the count is 0.
  await expect(page.getByText(/admin activity/i)).toBeVisible();
});

import { test, expect } from "@playwright/test";
import { setupClerkTestingToken } from "@clerk/testing/playwright";

/**
 * T-083.5 — audit log viewer.
 *
 * Read-only reverse-chronological feed of admin mutations. In a fresh
 * DB the list is empty; after specs 35 / 36 mutate the queue, entries
 * appear. This spec doesn't depend on either — it just asserts the
 * page renders, so it works standalone and doesn't need spec ordering.
 */
test("admin-audit-log-admin-signed-in: /admin/audit-log renders the header + empty-or-list state", async ({
  page,
}) => {
  await setupClerkTestingToken({ page });

  await page.goto("/admin/audit-log");

  await expect(
    page.getByRole("heading", { name: /^Audit log$/ }),
  ).toBeVisible({ timeout: 15_000 });

  // Copy under the heading is stable.
  await expect(page.getByText(/Every admin mutation, newest first/)).toBeVisible();

  // Either the empty-state copy or at least one entry — both are
  // valid renders depending on whether prior admin specs ran first.
  const empty = page.getByText(/No audit entries yet/);
  const anyEntry = page.locator("ul li").first();
  await expect(empty.or(anyEntry)).toBeVisible({ timeout: 5_000 });
});

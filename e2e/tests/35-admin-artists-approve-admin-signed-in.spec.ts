import { test, expect } from "@playwright/test";
import { setupClerkTestingToken } from "@clerk/testing/playwright";

/**
 * T-083 — admin approves a pending artist.
 *
 * Seed drops `dora-pending` (status='pending', slug 'dora-pending'). The
 * admin queue at `/admin/artists?status=pending` lists her; clicking
 * Approve fires the server action, which flips status to 'active' and
 * `router.refresh()` re-fetches. The row should then disappear from
 * the pending tab.
 *
 * Note: this spec mutates DB state (pending → active). CI's Postgres
 * service is ephemeral per job so this is safe there. Local re-runs
 * against a warm DB will find Dora already active — reset with
 * `PGPASSWORD=dev psql ... -f api/crates/api-search/tests/fixtures/seed.sql`
 * or restart the docker service.
 */
test("admin-artists-approve-admin-signed-in: pending row disappears after Approve", async ({
  page,
}) => {
  await setupClerkTestingToken({ page });

  await page.goto("/admin/artists?status=pending");
  await expect(
    page.getByRole("heading", { name: /^Artists$/ }),
  ).toBeVisible({ timeout: 15_000 });

  // Dora exists in the pending list.
  const doraLabel = page.getByText(/Dora Pending/);
  await expect(doraLabel).toBeVisible({ timeout: 10_000 });

  // Approve fires an optimistic action + toast + refresh.
  await page.getByRole("button", { name: /^Approve$/ }).first().click();

  // Sonner toast surfaces the success — cheap round-trip proof.
  await expect(page.getByText(/Dora Pending: Approved/)).toBeVisible({
    timeout: 10_000,
  });

  // After router.refresh(), the row is out of the pending tab.
  await expect(doraLabel).toHaveCount(0, { timeout: 10_000 });
});

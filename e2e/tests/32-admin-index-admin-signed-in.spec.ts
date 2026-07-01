import { test, expect } from "@playwright/test";
import { setupClerkTestingToken } from "@clerk/testing/playwright";

/**
 * T-083 — admin surface index.
 *
 * `/admin` layout gates every `/admin/*` route on `users.is_admin`.
 * The setup fixture (`admin.setup.ts`) signs up a user whose email
 * ends with `-admin+clerk_test@example.com`; that suffix is in
 * `WANDER_ADMIN_EMAIL_ALLOWLIST` (see `scripts/dev.sh` +
 * `.github/workflows/e2e.yml`), so the API's auto-promote path
 * flips `is_admin=true` on the first authenticated request.
 *
 * This spec smoke-tests that the seam is wired end-to-end: an admin
 * user sees the admin index with all three queue tiles.
 */
test("admin-index-admin-signed-in: /admin renders queue tiles + stats link", async ({
  page,
}) => {
  await setupClerkTestingToken({ page });

  await page.goto("/admin");
  await expect(page).toHaveURL(/\/admin$/);
  await expect(
    page.getByRole("heading", { name: /^Admin$/ }),
  ).toBeVisible({ timeout: 15_000 });

  // Three tiles — links to the sub-queues. Titles come from
  // `AdminTile` in `app/admin/page.tsx`.
  await expect(
    page.getByRole("heading", { name: "Artist applications" }),
  ).toBeVisible();
  await expect(
    page.getByRole("heading", { name: "Paused artists" }),
  ).toBeVisible();
  await expect(
    page.getByRole("heading", { name: "Image moderation" }),
  ).toBeVisible();

  // Audit log + stats deep links live in the footer nav.
  await expect(
    page.getByRole("link", { name: /View audit log/i }),
  ).toBeVisible();
  await expect(page.getByRole("link", { name: /^Stats/i })).toBeVisible();
});

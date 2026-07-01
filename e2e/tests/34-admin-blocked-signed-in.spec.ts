import { test, expect } from "@playwright/test";
import { setupClerkTestingToken } from "@clerk/testing/playwright";

/**
 * T-083 — admin gate blocks non-admins.
 *
 * Runs under the `chromium-authed` project (regular signed-in user,
 * NOT admin — the fresh Clerk email doesn't match the allowlist
 * suffix). The `/admin/layout.tsx` calls `notFound()` when the user's
 * `is_admin` flag is false, so the response should be the Next 404
 * page rather than the admin index.
 *
 * If someone accidentally weakens the gate to `if (!userId) notFound()`
 * (Clerk-only, ignoring is_admin), this spec fails immediately.
 */
test("admin-blocked-signed-in: non-admin signed-in user hitting /admin gets a 404", async ({
  page,
}) => {
  await setupClerkTestingToken({ page });

  const resp = await page.goto("/admin");
  expect(resp?.status()).toBe(404);

  // The admin index heading MUST NOT be visible — the 404 shell can
  // render whatever it wants, but it must not be the admin one.
  await expect(page.getByRole("heading", { name: /^Admin$/ })).toHaveCount(0);
});

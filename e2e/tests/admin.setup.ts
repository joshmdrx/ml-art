/**
 * Playwright "admin setup" project — sibling to `auth.setup.ts`.
 *
 * Signs up a fresh Clerk test user whose email ends with the suffix
 * `-admin+clerk_test@example.com`. The API's `is_seeded_admin_email`
 * check reads `WANDER_ADMIN_EMAIL_ALLOWLIST` (set in `scripts/dev.sh`
 * + `.github/workflows/e2e.yml`) and matches that suffix, so this
 * user is `is_admin = true` from the moment the first authenticated
 * request lands.
 *
 * Storage state is saved to `e2e/.auth/admin.json`; consumed by the
 * `chromium-admin` project. Tests opt in via filename pattern
 * `*admin-signed-in*.spec.ts`.
 */

import { test as setup, expect } from "@playwright/test";
import { clerkSetup, setupClerkTestingToken } from "@clerk/testing/playwright";
import { mkdirSync } from "node:fs";
import { dirname } from "node:path";

const AUTH_FILE = "e2e/.auth/admin.json";

// The trailing suffix is load-bearing — it MUST match the API's
// `WANDER_ADMIN_EMAIL_ALLOWLIST` entry. Prefix stays randomised so
// re-runs against the same Clerk dev instance don't collide on the
// "email already exists" branch.
function makeAdminEmail(): string {
  const stamp = Date.now().toString(36);
  const rand = Math.random().toString(36).slice(2, 6);
  return `e2e-${stamp}-${rand}-admin+clerk_test@example.com`;
}

setup("authenticate as a fresh admin user", async ({ page }) => {
  await clerkSetup();

  const email = makeAdminEmail();
  mkdirSync(dirname(AUTH_FILE), { recursive: true });

  await setupClerkTestingToken({ page });

  await page.goto("/sign-up");
  await page.waitForLoadState("networkidle");

  const emailInput = page.locator(
    'input[name="emailAddress"], input[type="email"]',
  );
  await expect(emailInput).toBeVisible({ timeout: 15_000 });
  await emailInput.fill(email);

  await page
    .getByRole("button", { name: /continue|sign up/i })
    .first()
    .click();

  const passwordInput = page.locator('input[name="password"]');
  if (await passwordInput.isVisible({ timeout: 5_000 }).catch(() => false)) {
    await passwordInput.fill("e2e-password-not-secret-just-needs-length");
    await page
      .getByRole("button", { name: /continue|sign up/i })
      .first()
      .click();
  }

  // Race an OTP screen vs a direct post-signup redirect — same shape
  // as auth.setup.ts.
  await Promise.race([
    page
      .locator('input[name="code"], input[autocomplete="one-time-code"]')
      .first()
      .waitFor({ state: "visible", timeout: 25_000 }),
    page.waitForURL((url) => !url.pathname.startsWith("/sign-up"), {
      timeout: 25_000,
    }),
  ]);

  const otpInput = page
    .locator('input[name="code"], input[autocomplete="one-time-code"]')
    .first();
  if (await otpInput.isVisible().catch(() => false)) {
    await otpInput.fill("424242");
    const verifyBtn = page
      .getByRole("button", { name: /verify|continue/i })
      .first();
    if (await verifyBtn.isVisible({ timeout: 2_000 }).catch(() => false)) {
      await verifyBtn.click();
    }
  }

  await page.waitForURL((url) => !url.pathname.startsWith("/sign-up"), {
    timeout: 20_000,
  });

  // Trigger the lazy upsert so is_admin gets flipped BEFORE we save
  // storageState. Any authenticated route works — /me is the cheapest.
  await page.goto("/me");
  await expect(page.getByText(/Authenticated\./)).toBeVisible({
    timeout: 15_000,
  });

  // Sanity: /admin should render for this user (non-admins get 404).
  // If this fails, the WANDER_ADMIN_EMAIL_ALLOWLIST env var isn't
  // reaching the API. Fail loudly here rather than in every downstream
  // admin spec.
  await page.goto("/admin");
  await expect(page.getByRole("heading", { name: /^Admin$/ })).toBeVisible({
    timeout: 15_000,
  });

  await page.context().storageState({ path: AUTH_FILE });
  // eslint-disable-next-line no-console
  console.log(`signed up admin ${email}, auth state → ${AUTH_FILE}`);
});

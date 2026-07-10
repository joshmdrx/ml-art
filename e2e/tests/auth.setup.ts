/**
 * Playwright "setup" project — runs once before the test suite to:
 *   1. Sign up a fresh user via Clerk's hosted sign-up
 *   2. Verify with Clerk's built-in test-email convention (any email
 *      matching `*+clerk_test@*` accepts the OTP `424242`)
 *   3. Save the resulting authenticated browser state (cookies, localStorage)
 *      so signed-in tests can start from a logged-in state
 *
 * Clerk's test-email feature is documented at:
 *   https://clerk.com/docs/testing/test-emails-and-phones
 *
 * The created user is real — they appear in Clerk's dev dashboard and in
 * our local `users` table on first authenticated request. We accept some
 * accumulation in the dev instance; cleanup script lives in TODO.
 */

import { test as setup, expect } from "@playwright/test";
import { clerkSetup, setupClerkTestingToken } from "@clerk/testing/playwright";
import { mkdirSync, writeFileSync } from "node:fs";
import { dirname } from "node:path";

const AUTH_FILE = "e2e/.auth/user.json";
// M-10 — the buyer's email, so marketplace specs can seed an order for
// this exact signed-in user via the `create-order` fixture.
const META_FILE = "e2e/.auth/user-meta.json";

// One user per run. Unique enough to avoid Clerk's "already exists" errors
// across re-runs; short enough to fit Clerk's email constraints.
function makeEmail(): string {
  const stamp = Date.now().toString(36);
  const rand = Math.random().toString(36).slice(2, 6);
  return `e2e-${stamp}-${rand}+clerk_test@example.com`;
}

setup("authenticate as a fresh test user", async ({ page }) => {
  // `clerkSetup` arms the testing token globally; `setupClerkTestingToken`
  // applies it to this Playwright page. Together they bypass Clerk's
  // Smart CAPTCHA / bot fingerprinting that would otherwise hang the
  // headless browser at the "Loading" state. Both must run in the same
  // worker (Playwright runs each test in its own worker process).
  await clerkSetup();

  const email = makeEmail();
  mkdirSync(dirname(AUTH_FILE), { recursive: true });

  await setupClerkTestingToken({ page });

  await page.goto("/sign-up");
  await page.waitForLoadState("networkidle");

  // Clerk's hosted sign-up form — selectors target their stable input names
  // / aria attributes. They evolve their UI occasionally; if these break,
  // run `npx playwright codegen http://localhost:3000/sign-up` to refresh.

  // Step 1: enter email
  const emailInput = page.locator(
    'input[name="emailAddress"], input[type="email"]'
  );
  await expect(emailInput).toBeVisible({ timeout: 15_000 });
  await emailInput.fill(email);

  // The "Continue" button. Clerk uses different labels depending on the
  // instance's auth-method configuration ("Continue" vs "Sign up"), so we
  // anchor on the form's submit role.
  await page
    .getByRole("button", { name: /continue|sign up/i })
    .first()
    .click();

  // If the instance is configured for email + password (most dev instances),
  // we'll see a password prompt next. Skip it if not.
  const passwordInput = page.locator('input[name="password"]');
  if (await passwordInput.isVisible({ timeout: 5_000 }).catch(() => false)) {
    await passwordInput.fill("e2e-password-not-secret-just-needs-length");
    await page
      .getByRole("button", { name: /continue|sign up/i })
      .first()
      .click();
  }

  // Step 2: verification. Two possible outcomes:
  //   (a) OTP screen appears — fill `424242` (test email auto-accepts).
  //   (b) Clerk skips OTP because the test email is pre-verified and
  //       lands us directly on the post-sign-up URL.
  // Race the two so whichever happens first wins.
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

  // After successful sign-up, Clerk redirects to the configured
  // fallback URL (see `NEXT_PUBLIC_CLERK_SIGN_UP_FALLBACK_REDIRECT_URL`).
  // We just need to see the user is signed in — TopNav's <UserButton>
  // is the simplest assertion.
  await page.waitForURL((url) => !url.pathname.startsWith("/sign-up"), {
    timeout: 20_000,
  });

  // Sanity: verify the API recognizes us. Hitting /me triggers the
  // lazy-upsert into our users table, which means storedState is genuinely
  // good for downstream tests.
  await page.goto("/me");
  await expect(page.getByText(/Authenticated\./)).toBeVisible({
    timeout: 15_000,
  });

  await page.context().storageState({ path: AUTH_FILE });
  writeFileSync(META_FILE, JSON.stringify({ email }), "utf8");
  // eslint-disable-next-line no-console
  console.log(`signed up as ${email}, auth state → ${AUTH_FILE}`);
});

import { test, expect } from "@playwright/test";
import { clerkSetup, setupClerkTestingToken } from "@clerk/testing/playwright";

/**
 * T-052c — anonymous follow-intent replayed after sign-in.
 *
 * The full cross-tier flow:
 *   1. Anonymous visitor on an artist page clicks Follow.
 *   2. Server action `queueAnonFollowAction` records the intent
 *      against the anon_id cookie.
 *   3. Client redirects to `/sign-in?redirect_url=/artists/<slug>`.
 *   4. User signs up (Clerk hosted flow).
 *   5. On the first authenticated request post-signup, the merge
 *      handler drains any queued anon follows onto the new user.
 *   6. Browser lands back on the artist page with the button reading
 *      "Following".
 *
 * Runs in the plain `chromium` project (no saved storageState) so the
 * signup happens fresh mid-test. Slower than the other specs (~30s
 * for the Clerk dance) but the only path that actually asserts the
 * queue-and-replay end-to-end.
 */

const SLUG = "demo-ukiyo-e";

function makeAnonFollowEmail(): string {
  const stamp = Date.now().toString(36);
  const rand = Math.random().toString(36).slice(2, 6);
  return `e2e-follow-${stamp}-${rand}+clerk_test@example.com`;
}

test("follow-anon-queue: anon click → sign-up → follow is applied post-merge", async ({
  page,
}) => {
  await clerkSetup();
  await setupClerkTestingToken({ page });

  await page.goto(`/artists/${SLUG}`);
  await expect(
    page.getByRole("heading", { name: /Ukiyo E Studio/i }),
  ).toBeVisible({ timeout: 15_000 });

  // Anon: button reads "Follow" and is not pressed.
  const anonFollow = page.getByRole("button", { name: /^Follow$/ });
  await expect(anonFollow).toBeVisible({ timeout: 10_000 });
  await expect(anonFollow).toHaveAttribute("aria-pressed", "false");

  // Click → server action queues, then router.push to /sign-in.
  await anonFollow.click();
  await expect(page).toHaveURL(/\/sign-in/, { timeout: 15_000 });

  // Drive the Clerk hosted signup — same dance as auth.setup.ts, kept
  // inline so this spec is self-contained.
  const email = makeAnonFollowEmail();
  const emailInput = page.locator(
    'input[name="emailAddress"], input[type="email"]',
  );
  // Clerk's sign-in view has a "Sign up" link. In some configs the
  // /sign-in URL is redirected straight to /sign-up when a redirect_url
  // is present; handle both by looking for the emailAddress input.
  if (!(await emailInput.isVisible({ timeout: 5_000 }).catch(() => false))) {
    await page
      .getByRole("link", { name: /Sign up/i })
      .first()
      .click();
  }
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

  await Promise.race([
    page
      .locator('input[name="code"], input[autocomplete="one-time-code"]')
      .first()
      .waitFor({ state: "visible", timeout: 25_000 }),
    page.waitForURL(new RegExp(`/artists/${SLUG}`), { timeout: 25_000 }),
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

  // Clerk lands us at the redirect_url — the artist page. The merge
  // handler runs on the first authenticated request (the artist-page
  // load); the queued follow is applied to the new user.
  await page.waitForURL(new RegExp(`/artists/${SLUG}`), { timeout: 20_000 });

  // Reload once to ensure the artist page re-fetches `is_following`
  // after the merge — the initial post-signup render can race the
  // background merge call.
  await page.reload();
  await expect(
    page.getByRole("button", { name: /Open user menu/i }),
  ).toBeVisible({ timeout: 15_000 });

  const following = page.getByRole("button", { name: /^Following$/ });
  await expect(following).toBeVisible({ timeout: 15_000 });
  await expect(following).toHaveAttribute("aria-pressed", "true");
});

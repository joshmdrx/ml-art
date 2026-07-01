/**
 * Playwright "artist setup" project — sibling to `auth.setup.ts` +
 * `admin.setup.ts`.
 *
 * Signs up a fresh Clerk user AND drives them through the onboarding
 * wizard to publish, producing a user with an `artists.user_id`
 * link. Storage state saves to `e2e/.auth/artist.json`; consumed by
 * the `chromium-artist` project. Tests opt in via filename pattern
 * `*artist-signed-in*.spec.ts`.
 *
 * Every run mints a brand-new artist row + slug. `artists.slug` is
 * UNIQUE across the whole table (not per-user), so the display name
 * carries a unique-per-run stamp to avoid collisions.
 */

import { test as setup, expect } from "@playwright/test";
import { clerkSetup, setupClerkTestingToken } from "@clerk/testing/playwright";
import { mkdirSync, writeFileSync } from "node:fs";
import { dirname } from "node:path";

const AUTH_FILE = "e2e/.auth/artist.json";
const META_FILE = "e2e/.auth/artist-meta.json";

function makeEmail(): string {
  const stamp = Date.now().toString(36);
  const rand = Math.random().toString(36).slice(2, 6);
  return `e2e-artist-${stamp}-${rand}+clerk_test@example.com`;
}

function makeDisplayName(): string {
  const stamp = Date.now().toString(36);
  const rand = Math.random().toString(36).slice(2, 6);
  return `E2E Artist ${stamp} ${rand}`;
}

setup("authenticate as a fresh onboarded artist", async ({ page }) => {
  await clerkSetup();
  const email = makeEmail();
  const displayName = makeDisplayName();
  mkdirSync(dirname(AUTH_FILE), { recursive: true });

  await setupClerkTestingToken({ page });

  // ─── Clerk signup ────────────────────────────────────────────────────
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

  // ─── Drive the onboarding wizard ────────────────────────────────────
  // /studio bounces a non-artist to /onboarding — a stable entry point
  // regardless of Clerk's post-signup redirect.
  await page.goto("/studio");
  await expect(page).toHaveURL(/\/onboarding(\?|$)/, { timeout: 15_000 });

  // Identity — the only required step.
  await expect(
    page.getByRole("heading", { name: /Let's start with who you are/i }),
  ).toBeVisible({ timeout: 15_000 });
  await page.getByLabel(/Display name/).fill(displayName);
  await page.getByLabel(/Location/).fill("Manchester, GB");
  await page.getByRole("button", { name: /Continue/ }).click();

  // Profile / artworks / locations — skip / continue.
  await page.waitForURL(/step=profile/, { timeout: 15_000 });
  await page.getByRole("link", { name: /Skip for now/i }).click();
  await page.waitForURL(/step=artworks/);
  await page.getByRole("link", { name: /^Continue$/ }).click();
  await page.waitForURL(/step=locations/);
  await page.getByRole("link", { name: /^Continue$/ }).click();

  // Review — publish.
  await page.waitForURL(/step=review/);
  await page.getByRole("button", { name: /Publish profile/ }).click();
  await page.waitForURL(/\/artists\//, { timeout: 15_000 });

  // Extract the slug the API minted so downstream specs can reach the
  // public artist page without another API round-trip.
  const match = page.url().match(/\/artists\/([^/?#]+)/);
  const slug = match?.[1];
  if (!slug) {
    throw new Error(`Couldn't extract artist slug from ${page.url()}`);
  }

  // Sanity: /studio now renders (no redirect to /onboarding). If this
  // fails the artist row didn't attach — fail loudly at setup time
  // rather than in every downstream spec.
  await page.goto("/studio");
  await expect(page).toHaveURL(/\/studio(\?|$)/, { timeout: 15_000 });

  await page.context().storageState({ path: AUTH_FILE });
  writeFileSync(
    META_FILE,
    JSON.stringify({ email, displayName, slug }, null, 2),
  );
  // eslint-disable-next-line no-console
  console.log(
    `signed up artist ${email} (${displayName} @ /artists/${slug}), auth state → ${AUTH_FILE}`,
  );
});

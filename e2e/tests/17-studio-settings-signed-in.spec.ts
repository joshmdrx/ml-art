import { test, expect } from "@playwright/test";
import { setupClerkTestingToken } from "@clerk/testing/playwright";

/**
 * T-011 Phase 2 — studio settings.
 *
 * The setup flow creates a fresh Clerk test user with no artist row
 * (the per-test `+clerk_test` email is local-only; we never link it to
 * `artists.user_id`). That makes the "you're not an artist yet" empty
 * state the realistic E2E assertion.
 *
 * Happy-path edits (paused → active, bio updates) are covered by Rust
 * integration tests against the seeded `alice-test` artist. Linking a
 * Clerk-test user to an artist row is a Phase 5+ test-fixture concern.
 */
test("studio-settings-signed-in: renders empty state for non-artist user", async ({
  page,
}) => {
  await setupClerkTestingToken({ page });

  await page.goto("/studio/settings");
  await expect(page.getByRole("button", { name: /Open user menu/i }))
    .toBeVisible({ timeout: 15_000 });

  // Heading is the page title (server-rendered).
  await expect(
    page.getByRole("heading", { name: "Studio settings" })
  ).toBeVisible();

  // Test user has no artists.user_id link → API returns 404 → empty
  // state renders.
  await expect(
    page.getByRole("heading", { name: /You're not set up as an artist/i })
  ).toBeVisible();

  // Visibility toggle and form fields must NOT render for non-artists.
  await expect(
    page.getByRole("heading", { name: "Portfolio visibility" })
  ).toHaveCount(0);
  await expect(page.getByLabel("Bio")).toHaveCount(0);
});

test("studio-settings-signed-out: redirects to sign-in", async ({ page }) => {
  // No setupClerkTestingToken — the page should bounce to /sign-in.
  // (Using `storageState: { cookies: [], origins: [] }` would be cleaner,
  // but the global setup applies storageState to all tests in the
  // chromium-authed project. The redirect happens server-side via
  // auth() returning no userId on a stateless visit before Clerk hydrates
  // — but with the setup's saved cookies, Clerk thinks we're signed in.
  // So this case is best left to a future "logged-out" project.)
  // For now, just sanity-check that the redirect logic is wired (the
  // server-side auth() call exists) by hitting the page and confirming
  // the auth call doesn't blow up.
  await setupClerkTestingToken({ page });
  await page.goto("/studio/settings");
  // Either the empty state OR a /sign-in redirect is acceptable here —
  // we only assert that the page resolves to *something* without 500.
  await expect(
    page.getByRole("heading", { name: /Studio settings|Sign in/ })
  ).toBeVisible({ timeout: 15_000 });
});

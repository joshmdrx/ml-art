import { test, expect } from "@playwright/test";
import { setupClerkTestingToken } from "@clerk/testing/playwright";

/**
 * `/studio/settings` — bio edit + persistence.
 *
 * Fixture artist visits their settings, edits the Bio, saves, and
 * reloads. The Bio textarea should reflect the new value after a
 * fresh render (i.e. the save actually hit the API + DB, not just
 * updated local state).
 *
 * Spec 17 already covers the /studio/settings ROUTE (redirect + form
 * render for a fresh Clerk user auto-provisioned as an artist). This
 * one is the read-after-write leg.
 */
test("studio-settings-edit-artist-signed-in: bio save persists across reload", async ({
  page,
}) => {
  await setupClerkTestingToken({ page });

  await page.goto("/studio/settings");
  await expect(page).toHaveURL(/\/studio\/settings/, { timeout: 15_000 });

  const bio = page.getByLabel(/^Bio$/);
  await expect(bio).toBeVisible({ timeout: 15_000 });

  // Unique per-run stamp so we can assert the exact value came back.
  const stamp = `${Date.now().toString(36)}-${Math.random().toString(36).slice(2, 6)}`;
  const value = `E2E bio ${stamp}`;

  await bio.fill(value);
  await page.getByRole("button", { name: /Save changes/ }).click();

  // Inline "Saved." acknowledgement fires when the mutation succeeds.
  await expect(page.getByText(/^Saved\.$/)).toBeVisible({ timeout: 10_000 });

  // Reload — the settings form re-fetches from the API. If the save
  // regressed to touch-local-state-only, the stamped value disappears.
  await page.reload();
  const bioAfter = page.getByLabel(/^Bio$/);
  await expect(bioAfter).toBeVisible({ timeout: 15_000 });
  await expect(bioAfter).toHaveValue(value);
});

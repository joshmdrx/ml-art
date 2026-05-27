import { test, expect } from "@playwright/test";
import { setupClerkTestingToken } from "@clerk/testing/playwright";

/**
 * T-029 — Save-modal membership awareness.
 *
 * Flow:
 *   1. Land on an artwork detail page (signed in)
 *   2. Open the Save modal; create a new collection inline (the click
 *      adds the artwork to it)
 *   3. Close the modal, re-open it on the same artwork
 *   4. The collection should now render as `aria-pressed="true"` —
 *      i.e. the API correctly returned `contains_artwork: true` via the
 *      `?artwork_id=` query path
 *
 * Without T-029 step 4 would fail because the modal would treat every
 * row as unchecked on every open.
 */
test("save-membership-signed-in: re-opening modal shows saved collection as pressed", async ({
  page,
}) => {
  await setupClerkTestingToken({ page });

  await page.goto("/search?q=ukiyo");
  await expect(page.getByRole("button", { name: /Open user menu/i })).toBeVisible({
    timeout: 15_000,
  });

  const firstTitle = page
    .locator("a[href^='/artworks/']:has-text('Untitled')")
    .first();
  await firstTitle.click();
  await expect(page).toHaveURL(/\/artworks\/[0-9a-f-]{36}/);
  await expect(page.getByRole("button", { name: /Open user menu/i })).toBeVisible({
    timeout: 15_000,
  });

  // 2a. First open: create a fresh collection inline (the create flow
  // also adds the current artwork — optimistic client-side update).
  await page.getByRole("button", { name: "Save to collection" }).click();
  let dialog = page.getByRole("dialog");
  await expect(dialog).toBeVisible({ timeout: 10_000 });

  const name = `E2E Membership ${Date.now().toString(36)}`;
  await dialog.getByLabel("New collection name").fill(name);
  await dialog.getByRole("button", { name: "Create" }).click();

  const newRow = dialog.getByRole("button", { name: new RegExp(name) });
  await expect(newRow).toBeVisible({ timeout: 10_000 });

  // Close the modal — Radix routes Escape to onOpenChange(false).
  await page.keyboard.press("Escape");
  await expect(dialog).not.toBeVisible({ timeout: 5_000 });

  // 3. Re-open. This time the modal re-fetches `/v1/me/collections?artwork_id=…`
  // and seeds `saved` from `contains_artwork` — the only path that exercises
  // T-029 server-side.
  await page.getByRole("button", { name: "Save to collection" }).click();
  dialog = page.getByRole("dialog");
  await expect(dialog).toBeVisible({ timeout: 10_000 });

  // 4. Assert the row is pressed (i.e. server reported contains_artwork: true).
  const persistedRow = dialog.getByRole("button", { name: new RegExp(name) });
  await expect(persistedRow).toBeVisible({ timeout: 10_000 });
  await expect(persistedRow).toHaveAttribute("aria-pressed", "true");
});

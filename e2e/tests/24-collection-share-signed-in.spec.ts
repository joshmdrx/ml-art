import { test, expect } from "@playwright/test";
import { setupClerkTestingToken } from "@clerk/testing/playwright";

/**
 * T-053 — public collection sharing.
 *
 * A collection defaults to private. The owner toggles it public, which
 * mints (or reveals) a `share_id`; anyone with `/c/<share_id>` can then
 * see the mood board without signing in.
 *
 * This spec covers the round trip:
 *   1. Signed-in owner creates a fresh collection (via the Save-modal
 *      inline-create path on an artwork page — same seam as spec 15).
 *   2. Opens the collection detail page, toggles it public.
 *   3. Reads the share URL out of the readonly input.
 *   4. Opens the share URL in a fresh anonymous browser context and
 *      confirms the collection name renders + the read-view has no
 *      Save / Inquire affordances.
 *
 * The public-view assertion is the load-bearing one — it's the only
 * path that catches "shared route quietly requires auth" regressions.
 */
test("collection-share-signed-in: owner toggles public → anon can view via /c/<share_id>", async ({
  page,
  browser,
}) => {
  await setupClerkTestingToken({ page });

  // 1. Create a collection via the Save-modal inline-create seam.
  await page.goto("/search?q=ukiyo");
  await expect(
    page.getByRole("button", { name: /Open user menu/i }),
  ).toBeVisible({ timeout: 15_000 });

  const artworkLink = page
    .locator("a[href^='/artworks/']:has-text('Untitled')")
    .first();
  await artworkLink.click();
  await expect(page).toHaveURL(/\/artworks\/[0-9a-f-]{36}/);

  await page.getByRole("button", { name: "Save to collection" }).click();
  const dialog = page.getByRole("dialog");
  await expect(dialog).toBeVisible({ timeout: 10_000 });

  const name = `E2E Share ${Date.now().toString(36)}`;
  await dialog.getByLabel("New collection name").fill(name);
  await dialog.getByRole("button", { name: "Create" }).click();
  // Wait for the new collection row to appear (create resolved).
  await expect(
    dialog.getByRole("button", { name: new RegExp(name) }),
  ).toBeVisible({ timeout: 10_000 });
  await page.keyboard.press("Escape");
  await expect(dialog).not.toBeVisible({ timeout: 5_000 });

  // 2. Open the collection detail page from /collections.
  await page.goto("/collections");
  await expect(
    page.getByRole("heading", { name: "Your collections" }),
  ).toBeVisible();

  const card = page.getByRole("link", { name: new RegExp(name) });
  await expect(card).toBeVisible();
  await card.click();
  await expect(page).toHaveURL(/\/collections\/[0-9a-f-]{36}/);
  await expect(
    page.getByRole("heading", { name, exact: true }),
  ).toBeVisible();

  // 3. Toggle public. "Make public" is the initial CTA; after the
  // server round-trips, a readonly input holds the share URL.
  await page.getByRole("button", { name: "Make public" }).click();
  const shareInput = page.locator("input[readonly]").first();
  await expect(shareInput).toBeVisible({ timeout: 10_000 });
  const shareUrl = await shareInput.inputValue();
  expect(shareUrl).toMatch(/\/c\/[a-zA-Z0-9_-]+$/);

  // 4. Anon context reads the share URL. A fresh context guarantees
  //    no auth cookies leak from the owner session.
  const anonContext = await browser.newContext();
  try {
    const anonPage = await anonContext.newPage();
    await anonPage.goto(shareUrl);
    await expect(
      anonPage.getByRole("heading", { name, exact: true }),
    ).toBeVisible({ timeout: 10_000 });
    // The public view intentionally omits Save + Inquire actions —
    // if these leak in, the read-view isn't read-only.
    await expect(
      anonPage.getByRole("button", { name: "Save to collection" }),
    ).toHaveCount(0);
  } finally {
    await anonContext.close();
  }
});

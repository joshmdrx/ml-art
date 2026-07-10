import { readFileSync } from "node:fs";
import { test, expect } from "@playwright/test";
import { setupClerkTestingToken } from "@clerk/testing/playwright";
import { createArtwork, enablePayouts, makeSellable } from "../lib/fixtures";

/**
 * M-05 — the Buy button only shows when a work is *purchasable*
 * (available + priced in GBP + dimensions + weight + ships-from + the
 * artist has Stripe payouts enabled). This spec drives that gate with
 * the M-10 fixtures instead of the real Stripe onboarding:
 *
 *   - enablePayouts(artist) flips the artist "as if" KYC completed.
 *   - one artwork is made fully sellable → Buy shows.
 *   - a second artwork lacks the fields → Buy hidden, only Inquire.
 *
 * Runs in the artist project (the artist views their own works). The
 * `purchasable` flag is viewer-independent, so this is a valid check.
 */
test("buy-button-artist-signed-in: Buy shows on a sellable work, hidden otherwise", async ({
  page,
}) => {
  await setupClerkTestingToken({ page });

  const meta = JSON.parse(
    readFileSync("e2e/.auth/artist-meta.json", "utf8"),
  ) as { slug: string };

  await enablePayouts(meta.slug);

  const sellable = await createArtwork({
    artistSlug: meta.slug,
    title: "E2E sellable work",
    priceCents: 50_000,
    currency: "GBP",
    dimensions: { width_cm: 30, height_cm: 40 },
    status: "published",
  });
  await makeSellable(sellable.id);

  const inquiryOnly = await createArtwork({
    artistSlug: meta.slug,
    title: "E2E inquiry-only work",
    priceCents: 50_000,
    currency: "GBP",
    status: "published",
  });

  // Sellable → Buy now visible.
  await page.goto(`/artworks/${sellable.id}`);
  await expect(
    page.getByRole("link", { name: /^Buy now$/ }),
  ).toBeVisible({ timeout: 15_000 });

  // Not sellable → no Buy button; Inquire is the CTA.
  await page.goto(`/artworks/${inquiryOnly.id}`);
  await expect(page.getByRole("button", { name: /^Inquire$/ })).toBeVisible({
    timeout: 15_000,
  });
  await expect(page.getByRole("link", { name: /^Buy now$/ })).toHaveCount(0);
});

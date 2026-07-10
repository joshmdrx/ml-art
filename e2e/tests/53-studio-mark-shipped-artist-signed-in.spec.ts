import { readFileSync } from "node:fs";
import { test, expect } from "@playwright/test";
import { setupClerkTestingToken } from "@clerk/testing/playwright";
import { createArtwork, createOrder } from "../lib/fixtures";

/**
 * M-06 — the artist marks a paid order shipped.
 *
 * Seeds a paid order (buyer = the seed's Test User) against a fresh
 * artwork owned by the fixture artist via the M-10 `create-order` seam,
 * then drives `/studio/orders/[id]`: fill carrier + tracking, submit,
 * assert the success toast (proof the paid→shipped transition landed).
 */
test("studio-mark-shipped-artist-signed-in: paid order flips to shipped", async ({
  page,
}) => {
  await setupClerkTestingToken({ page });

  const meta = JSON.parse(
    readFileSync("e2e/.auth/artist-meta.json", "utf8"),
  ) as { slug: string };

  const artwork = await createArtwork({
    artistSlug: meta.slug,
    title: "E2E ship-test work",
    priceCents: 50_000,
    currency: "GBP",
    status: "published",
  });
  // Buyer identity doesn't matter here — the fixture attaches any user.
  const order = await createOrder({
    artworkId: artwork.id,
    status: "paid",
  });

  await page.goto(`/studio/orders/${order.id}`);
  await expect(
    page.getByRole("heading", { name: /E2E ship-test work/ }),
  ).toBeVisible({ timeout: 15_000 });

  await page.getByLabel(/Carrier/i).fill("Royal Mail");
  await page.getByLabel(/Tracking number/i).fill("RM123456789GB");
  await page.getByRole("button", { name: /^Mark as shipped$/ }).click();

  await expect(page.getByText(/Marked as shipped/i)).toBeVisible({
    timeout: 10_000,
  });
});

import { readFileSync } from "node:fs";
import { test, expect } from "@playwright/test";
import { setupClerkTestingToken } from "@clerk/testing/playwright";
import { createArtwork, createInquiry } from "../lib/fixtures";

/**
 * T-074 — unread inquiry badge on the studio sidebar.
 *
 * `/v1/studio/me.unread_inquiry_count` counts inquiries where
 * `delivered_at IS NOT NULL AND read_at IS NULL`. When > 0 the
 * StudioSidebar's Inquiries item renders `<UnreadBadge>` with an
 * `aria-label="<n> unread inquiries"`.
 *
 * This spec uses the T-069 test-fixture seam to seed one delivered
 * inquiry against a fresh artwork owned by the fixture artist, then
 * navigates to /studio and asserts the badge is present.
 */
test("unread-inquiry-badge-artist-signed-in: 1 delivered inquiry surfaces on the Inquiries nav", async ({
  page,
}) => {
  await setupClerkTestingToken({ page });

  const meta = JSON.parse(
    readFileSync("e2e/.auth/artist-meta.json", "utf8"),
  ) as { slug: string };

  const artwork = await createArtwork({
    artistSlug: meta.slug,
    title: "E2E badge test artwork",
    status: "published",
    withImage: true,
  });
  await createInquiry({
    artworkId: artwork.id,
    fromEmail: "e2e-badge@example.com",
    message: "Badge fixture inquiry.",
    state: "delivered",
  });

  await page.goto("/studio");
  await expect(
    page.getByRole("navigation", { name: /Studio navigation/i }),
  ).toBeVisible({ timeout: 15_000 });

  // The badge is a span with aria-label="<n> unread inquiries". Match
  // any count ≥ 1 — the fixture inserts 1 but a prior CI run can
  // leave residual state, so we're tolerant of >1.
  const badge = page.getByLabel(/^\d+ unread inquiries$/);
  await expect(badge).toBeVisible({ timeout: 10_000 });
});

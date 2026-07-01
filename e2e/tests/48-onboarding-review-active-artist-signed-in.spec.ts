import { readFileSync } from "node:fs";
import { test, expect } from "@playwright/test";
import { setupClerkTestingToken } from "@clerk/testing/playwright";

/**
 * Onboarding wizard review step, "already active" branch (T-012 P1).
 *
 * The default review step ships a "Publish profile" mutation button.
 * For an artist whose row is already `status='active'`, the same
 * step renders "View your profile →" instead — the wizard is
 * reachable for edits, but re-publishing is a no-op.
 *
 * Fixture artist is already active (setup drives publish). Visiting
 * `/onboarding?step=review` should hit the `alreadyActive` branch.
 */
test("onboarding-review-active-artist-signed-in: review step shows 'View your profile', not Publish", async ({
  page,
}) => {
  await setupClerkTestingToken({ page });

  // Read the fixture's slug so we can assert the target URL of the
  // "View your profile" link.
  const meta = JSON.parse(
    readFileSync("e2e/.auth/artist-meta.json", "utf8"),
  ) as { slug: string; displayName: string };

  await page.goto("/onboarding?step=review");

  // The active-branch link, not the publish button.
  const viewLink = page.getByRole("link", { name: /View your profile/i });
  await expect(viewLink).toBeVisible({ timeout: 15_000 });
  await expect(viewLink).toHaveAttribute("href", `/artists/${meta.slug}`);

  // Publish button must NOT be present in this state — that's the
  // whole point of the alreadyActive branch.
  await expect(
    page.getByRole("button", { name: /Publish profile/ }),
  ).toHaveCount(0);

  await viewLink.click();
  await expect(page).toHaveURL(new RegExp(`/artists/${meta.slug}`));
  await expect(
    page.getByRole("heading", { name: new RegExp(meta.displayName) }),
  ).toBeVisible({ timeout: 10_000 });
});

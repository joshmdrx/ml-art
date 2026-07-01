import { test, expect } from "@playwright/test";
import { setupClerkTestingToken } from "@clerk/testing/playwright";

/**
 * T-052 — follow / unfollow an artist.
 *
 * The signed-in branch of `FollowButton` optimistically flips state
 * client-side and calls a server action; on reload the initial
 * `is_following` from `/v1/artists/{slug}` is what actually shows.
 * So the persistence assertion (reload → still "Following") is the
 * one that catches "we flipped locally but never wrote" regressions.
 *
 * Fresh Clerk user → guaranteed not-following on first visit.
 */
test("follow-signed-in: click Follow, reload, still Following, then unfollow", async ({
  page,
}) => {
  await setupClerkTestingToken({ page });

  await page.goto("/artists/demo-ukiyo-e");
  // Wait for the Clerk-gated TopNav to hydrate — the FollowButton uses
  // `useAuth()` and stays disabled until `isLoaded === true`.
  await expect(
    page.getByRole("button", { name: /Open user menu/i }),
  ).toBeVisible({ timeout: 15_000 });

  const followBtn = page.getByRole("button", { name: /^Follow$/ });
  await expect(followBtn).toBeVisible({ timeout: 10_000 });
  await expect(followBtn).toHaveAttribute("aria-pressed", "false");

  await followBtn.click();

  const following = page.getByRole("button", { name: /^Following$/ });
  await expect(following).toBeVisible({ timeout: 10_000 });
  await expect(following).toHaveAttribute("aria-pressed", "true");

  // Reload — the only path that proves the follow actually persisted.
  await page.reload();
  await expect(
    page.getByRole("button", { name: /Open user menu/i }),
  ).toBeVisible({ timeout: 15_000 });
  const persisted = page.getByRole("button", { name: /^Following$/ });
  await expect(persisted).toBeVisible({ timeout: 10_000 });
  await expect(persisted).toHaveAttribute("aria-pressed", "true");

  // Unfollow — clean up so re-runs on the same Clerk user (if the
  // storageState survived) don't start already-following.
  await persisted.click();
  const reverted = page.getByRole("button", { name: /^Follow$/ });
  await expect(reverted).toBeVisible({ timeout: 10_000 });
  await expect(reverted).toHaveAttribute("aria-pressed", "false");
});

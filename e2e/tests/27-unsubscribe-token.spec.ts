import { test, expect } from "@playwright/test";

/**
 * T-068 — one-click unsubscribe error paths.
 *
 * `/u/confirm` is the landing after clicking an email footer link;
 * `/u/[token]` GET redirects to `/u/confirm?token=…`. The confirm page
 * calls the API server-side and renders either a success acknowledgement
 * or an error state.
 *
 * We cover the error states — the success path needs a real JWT minted
 * by the API (kind + user_id + expiry), which the E2E harness doesn't
 * currently mint. Success is exercised by Rust integration tests +
 * covered manually via the smoke suite.
 *
 * The error copy is what a real user actually sees when a link is
 * malformed or their email client corrupted the token in transit —
 * if we ever revert to leaking raw error messages here, this catches
 * it.
 */

test("unsubscribe-token: /u/confirm with no token renders the friendly error copy", async ({
  page,
}) => {
  await page.goto("/u/confirm");
  await expect(
    page.getByText(/Unsubscribe link missing its token/i),
  ).toBeVisible({ timeout: 10_000 });
});

test("unsubscribe-token: /u/confirm?token=bogus renders the invalid-link copy", async ({
  page,
}) => {
  await page.goto("/u/confirm?token=this.is.not.a.jwt");
  await expect(
    page.getByText(
      /This unsubscribe link isn't valid or has expired|isn.t valid or has expired/i,
    ),
  ).toBeVisible({ timeout: 10_000 });
});

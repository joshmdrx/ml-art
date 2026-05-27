import { test, expect } from "@playwright/test";

/**
 * `/inquiries/verify/<token>` for an unknown token returns the
 * not-found state (server-rendered — no Clerk dependency, no auth).
 */
test("verify page with unknown token renders not-found message", async ({
  page,
}) => {
  await page.goto("/inquiries/verify/this-token-was-never-issued");

  await expect(
    page.getByRole("heading", { name: /Link doesn.t look right/i })
  ).toBeVisible();
  await expect(
    page.getByText(/can't find an inquiry matching this link/i)
  ).toBeVisible();
});

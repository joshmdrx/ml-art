import { test, expect } from "@playwright/test";

/**
 * Anonymous Inquire flow end-to-end.
 *
 *  1. Find an artwork from search (any one with an active artist works)
 *  2. Click "Inquire" — modal opens
 *  3. Fill name / email / message → submit
 *  4. Success state shows "Check your inbox" + a dev-only verify link
 *  5. Visit the verify link → "Sent." confirmation page
 *
 * Stays in the signed-out cookie state — no Clerk session involved.
 *
 * Note: this exercises a real Rust handler path and writes a real row to
 * the local Postgres. Each test run leaves an inquiry behind in the seed
 * data; that's fine for v0.
 */
test("anonymous inquiry: submit → check inbox → verify link delivers", async ({
  page,
}) => {
  // 1. Land on a deterministic artwork by using the keyword path.
  await page.goto("/search?q=ukiyo");
  const firstTitle = page
    .locator("a[href^='/artworks/']:has-text('Untitled')")
    .first();
  await firstTitle.click();
  await expect(page).toHaveURL(/\/artworks\/[0-9a-f-]{36}/);

  // 2. Open the Inquire modal.
  await page.getByRole("button", { name: "Inquire" }).click();
  const dialog = page.getByRole("dialog");
  await expect(dialog).toBeVisible();
  await expect(dialog.getByText(/This goes directly to/i)).toBeVisible();

  // 3. Fill it in. Signed-out → all fields are editable.
  await dialog.getByLabel("Name").fill("Test Stranger");
  await dialog.getByLabel("Email").fill("e2e@example.com");
  await dialog
    .getByLabel("Message")
    .fill("Loved this work — is it still available?");
  await dialog.getByRole("button", { name: "Send inquiry" }).click();

  // 4. Post-submit state.
  await expect(
    dialog.getByText(/Check your inbox at/i)
  ).toBeVisible({ timeout: 10_000 });
  await expect(
    dialog.getByText("e2e@example.com")
  ).toBeVisible();

  // 5. Follow the dev-only verify link rendered into the modal.
  const verifyLink = dialog.getByRole("link", { name: /\/inquiries\/verify\// });
  await expect(verifyLink).toBeVisible();
  await verifyLink.click();

  await expect(page).toHaveURL(/\/inquiries\/verify\//);
  await expect(page.getByRole("heading", { name: "Sent." })).toBeVisible();
});

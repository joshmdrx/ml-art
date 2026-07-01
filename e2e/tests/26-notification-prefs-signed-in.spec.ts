import { test, expect } from "@playwright/test";
import { setupClerkTestingToken } from "@clerk/testing/playwright";

/**
 * T-068 — notification preferences.
 *
 * The prefs page is a server-rendered read of `/v1/me/notifications`
 * feeding a client form that flips per-kind via a server action.
 * Optimistic-update semantics: local flip → server persists → revert
 * on error.
 *
 * We assert:
 *   1. Page loads for a signed-in user (auto-provisioned prefs row).
 *   2. Toggling the global switch changes its aria-checked.
 *   3. Reloading picks up the persisted state (the load-bearing bit).
 *
 * Toggle is a `role="switch"` with `aria-checked` — Playwright's
 * `getByRole("switch")` targets it cleanly by its label.
 */
test("notification-prefs-signed-in: toggle global switch, reload, persists", async ({
  page,
}) => {
  await setupClerkTestingToken({ page });

  await page.goto("/me/settings/notifications");
  await expect(
    page.getByRole("heading", { name: /Email notifications/i }),
  ).toBeVisible({ timeout: 15_000 });

  const globalSwitch = page.getByRole("switch", {
    name: /All notification emails/i,
  });
  await expect(globalSwitch).toBeVisible({ timeout: 10_000 });

  const initial = await globalSwitch.getAttribute("aria-checked");
  expect(initial === "true" || initial === "false").toBeTruthy();
  const flipped = initial === "true" ? "false" : "true";

  await globalSwitch.click();
  await expect(globalSwitch).toHaveAttribute("aria-checked", flipped);

  // Reload — the persistence assertion. Without T-068 wiring, a
  // successful client flip could disappear on refresh.
  await page.reload();
  const afterReload = page.getByRole("switch", {
    name: /All notification emails/i,
  });
  await expect(afterReload).toBeVisible({ timeout: 10_000 });
  await expect(afterReload).toHaveAttribute("aria-checked", flipped);

  // Reset so the fresh Clerk user's DB row doesn't drift across
  // sequential runs (harmless either way, but tidy).
  await afterReload.click();
  await expect(afterReload).toHaveAttribute("aria-checked", initial ?? "true");
});

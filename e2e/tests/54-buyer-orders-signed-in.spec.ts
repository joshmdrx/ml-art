import { readFileSync } from "node:fs";
import { test, expect } from "@playwright/test";
import { setupClerkTestingToken } from "@clerk/testing/playwright";
import { createOrder } from "../lib/fixtures";

/**
 * M-11 — the buyer's order-history page.
 *
 * Seeds an order for *this* signed-in buyer (email persisted by
 * auth.setup → user-meta.json) against a seeded artwork, then asserts
 * `/orders` lists it with a link to the order confirmation page.
 */
test("buyer-orders-signed-in: /orders lists the buyer's order", async ({
  page,
}) => {
  await setupClerkTestingToken({ page });

  const meta = JSON.parse(
    readFileSync("e2e/.auth/user-meta.json", "utf8"),
  ) as { email: string };

  // Seed's "Blue Morning" by Alice Test.
  const order = await createOrder({
    buyerEmail: meta.email,
    artworkId: "bbb11111-1111-1111-1111-111111111111",
    status: "paid",
  });

  await page.goto("/orders");
  await expect(
    page.getByRole("heading", { name: /Your orders/ }),
  ).toBeVisible({ timeout: 15_000 });

  // The row links to the confirmation page for this order.
  await expect(
    page.locator(`a[href="/orders/${order.id}"]`),
  ).toBeVisible({ timeout: 10_000 });
});

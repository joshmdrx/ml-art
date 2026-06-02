import { test, expect } from "@playwright/test";
import { setupClerkTestingToken } from "@clerk/testing/playwright";

/**
 * T-033 — anon→user merge bridge.
 *
 * The signed-in storageState only persists localStorage + cookies,
 * NOT sessionStorage. So on every fresh Playwright page open, the
 * `mlart_anon_merged` marker is absent and the <AnonymousMergeBridge/>
 * mounted in the root layout fires `POST /api/me/merge-anonymous`
 * once.
 *
 * The merge body itself is exhaustively unit + integration tested at
 * the Rust tier (`merge_anonymous_test.rs`, 8 tests). This spec
 * specifically guards the *web bridge*:
 *
 *   - the route handler exists + returns 200 for a signed-in caller
 *   - the bridge fires exactly once per browser session
 *   - the sessionStorage marker is set on success and survives
 *     subsequent navigations
 *
 * If a refactor breaks any of those, this test fails fast.
 */

test("anon-merge-signed-in: bridge fires once and sets sessionStorage marker", async ({
  page,
}) => {
  await setupClerkTestingToken({ page });

  // Watch every POST to the merge bridge.
  const mergeRequests: string[] = [];
  page.on("request", (req) => {
    if (
      req.method() === "POST" &&
      req.url().includes("/api/me/merge-anonymous")
    ) {
      mergeRequests.push(req.url());
    }
  });

  // Hit any signed-in surface. The bridge is mounted in the root
  // layout so any page works; /me is the smallest server-rendered
  // payload that proves we're authenticated.
  await page.goto("/me");
  await expect(page.getByText(/Authenticated\./)).toBeVisible({
    timeout: 15_000,
  });

  // Give the client `useEffect` a tick to fire after hydration. The
  // bridge is fire-and-forget — we wait on the network call rather
  // than a UI side-effect.
  await page.waitForRequest(
    (req) =>
      req.method() === "POST" && req.url().includes("/api/me/merge-anonymous"),
    { timeout: 10_000 }
  );

  expect(mergeRequests.length).toBe(1);

  // Marker should now be set; the next navigation should NOT refire.
  const marker = await page.evaluate(() =>
    window.sessionStorage.getItem("mlart_anon_merged")
  );
  expect(marker).toBe("1");

  // Cross-nav: marker survives client-side navigation within the same
  // tab. If the bridge re-fires, that's a regression — extra API calls
  // for no gain.
  await page.goto("/collections");
  // Wait for the page to settle; sessionStorage persists across SPA
  // navigations within the same browsing context.
  await page.waitForLoadState("networkidle");
  expect(mergeRequests.length).toBe(1);
});

test("anon-merge-signed-in: bridge returns 200 with merge counts", async ({
  page,
}) => {
  await setupClerkTestingToken({ page });

  // Intercept the response shape so a future refactor that changes
  // the bridge contract gets caught here, not in production.
  const responsePromise = page.waitForResponse(
    (res) =>
      res.url().includes("/api/me/merge-anonymous") && res.request().method() === "POST",
    { timeout: 15_000 }
  );

  await page.goto("/me");
  await expect(page.getByText(/Authenticated\./)).toBeVisible({
    timeout: 15_000,
  });

  const response = await responsePromise;
  expect(response.status()).toBe(200);
  const body = await response.json();
  // The signed-in user has no anonymous trail (storageState was
  // captured fresh in auth.setup.ts), so the merge is a clean zero.
  // We only assert the *shape* — the counts may be > 0 if a previous
  // session left rows around in dev.
  expect(typeof body.uploads_merged).toBe("number");
  expect(typeof body.events_merged).toBe("number");
});

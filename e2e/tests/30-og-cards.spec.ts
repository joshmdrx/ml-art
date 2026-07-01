import { test, expect } from "@playwright/test";

/**
 * T-051 — per-artwork + per-artist OG cards.
 *
 * `app/artworks/[id]/opengraph-image.tsx` and
 * `app/artists/[slug]/opengraph-image.tsx` use Next's file-based
 * convention: Next auto-injects a `<meta property="og:image">` tag
 * pointing at the same route + `/opengraph-image`, and the route
 * renders a 1200x630 PNG via `next/og` (Satori + Resvg).
 *
 * The risk this catches:
 *   - The meta tag disappears (someone overrides `generateMetadata`
 *     and forgets to spread the auto-injected fields)
 *   - The OG route 500s (Satori chokes on a font or the wrapped image
 *     URL) — a bad social share for every artwork/artist on the site
 *
 * Assertion: fetch the page, read the meta URL, `GET` it, expect a 200
 * with `image/*` content type. `next/og` first-paint can be slow —
 * generous timeout.
 */

async function assertOgImage(
  page: import("@playwright/test").Page,
  pagePath: string,
) {
  await page.goto(pagePath);
  const metaUrl = await page
    .locator('meta[property="og:image"]')
    .first()
    .getAttribute("content");
  expect(metaUrl, `og:image meta on ${pagePath}`).toBeTruthy();
  expect(metaUrl!).toMatch(/opengraph-image/);

  const resp = await page.request.get(metaUrl!, { timeout: 30_000 });
  expect(resp.status(), `OG image status for ${pagePath}`).toBe(200);
  expect(resp.headers()["content-type"] ?? "").toMatch(/^image\//);
}

test("og-cards: artwork detail page emits an og:image that resolves to an image", async ({
  page,
}) => {
  // Reach a real artwork via search — id is a UUID we can't hardcode.
  await page.goto("/search?q=ukiyo");
  const link = page
    .locator("a[href^='/artworks/']:has-text('Untitled')")
    .first();
  const href = await link.getAttribute("href");
  expect(href).toMatch(/^\/artworks\/[0-9a-f-]{36}$/);

  await assertOgImage(page, href!);
});

test("og-cards: artist page emits an og:image that resolves to an image", async ({
  page,
}) => {
  await assertOgImage(page, "/artists/demo-ukiyo-e");
});

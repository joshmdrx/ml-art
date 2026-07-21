import { test, expect } from "@playwright/test";

// /sitemap.xml points crawlers at canonical content routes (the positive
// counterpart to robots.txt disallowing /search). See web/src/app/sitemap.ts.
test("sitemap.xml is valid XML with the static content routes", async ({
  request,
}) => {
  const res = await request.get("/sitemap.xml");
  expect(res.status()).toBe(200);
  expect(res.headers()["content-type"]).toContain("xml");

  const body = await res.text();
  expect(body).toContain("<urlset");
  // Static routes are always present even if the API scan degrades.
  for (const path of ["/", "/neighborhoods", "/for-artists", "/about"]) {
    expect(body).toContain(`<loc>https://wander.gallery${path}</loc>`);
  }
  // Must NOT advertise the disallowed faceted search surface.
  expect(body).not.toContain("/search</loc>");
});

test("search page is noindex", async ({ page }) => {
  await page.goto("/search");
  const robots = page.locator('meta[name="robots"]');
  await expect(robots).toHaveAttribute("content", /noindex/);
});

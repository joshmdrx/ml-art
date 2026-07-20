import { test, expect } from "@playwright/test";

// `/robots.txt` disallows the unbounded `/search` param space so
// crawlers stop fanning out into the API + Neon. See web/src/app/robots.ts.
test("robots.txt disallows /search and authed routes", async ({ request }) => {
  const res = await request.get("/robots.txt");
  expect(res.status()).toBe(200);

  const body = await res.text();
  expect(body).toContain("User-Agent: *");
  expect(body).toContain("Disallow: /search");
  for (const path of ["/admin", "/studio", "/me", "/orders", "/onboarding"]) {
    expect(body).toContain(`Disallow: ${path}`);
  }
});

test("robots.txt keeps content routes crawlable", async ({ request }) => {
  const res = await request.get("/robots.txt");
  const body = await res.text();

  // These earn SEO and must not be disallowed.
  for (const path of ["/artists", "/collections", "/neighborhoods"]) {
    expect(body).not.toContain(`Disallow: ${path}`);
  }
});

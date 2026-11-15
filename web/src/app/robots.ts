import type { MetadataRoute } from "next";

/**
 * `/robots.txt` for wander.gallery.
 *
 * `/search` carries an unbounded faceted param space
 * (`?artist=&location=&map=&bbox=`); crawlers enumerating it drove a
 * search-render fan-out into the Rust API and Neon with zero real
 * users. Disallowing the whole path stops compliant bots at source —
 * the SEO value lives in the content routes (`/artists/[slug]`,
 * `/collections/[id]`, `/neighborhoods/[slug]`), which stay crawlable.
 * robots.txt is advisory, so this is paired with a WAF user-agent
 * block for the bots that ignore it.
 *
 * `/preview` (T-086) holds private, token-gated draft pages that must
 * never be indexed; the page also sets `noindex`.
 *
 * The authed routes are hygiene, not cost: they redirect to Clerk
 * sign-in, so disallowing them just keeps sign-in pages out of the
 * index and saves crawlers a wasted redirect render.
 */
export default function robots(): MetadataRoute.Robots {
  return {
    rules: {
      userAgent: "*",
      allow: "/",
      disallow: [
        "/search",
        "/admin",
        "/studio",
        "/me",
        "/orders",
        "/onboarding",
        // T-086 — private draft previews; also `noindex` on the page.
        "/preview",
      ],
    },
    sitemap: "https://wander.gallery/sitemap.xml",
  };
}

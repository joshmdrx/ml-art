import type { MetadataRoute } from "next";
import { listNeighborhoods, searchArtworks } from "@/lib/api";
import { reportError } from "@/lib/reportError";

/**
 * `/sitemap.xml` for wander.gallery.
 *
 * The positive counterpart to `robots.ts`: robots tells crawlers what
 * NOT to crawl (the unbounded `/search` facet space); the sitemap points
 * them at the canonical content URLs we DO want indexed — artist
 * portfolios, artwork detail pages, and neighborhoods. Channelling good
 * bots here is what keeps them off `/search` (see the 2026-07 crawler
 * cost incident in `decisions.md`).
 *
 * Regenerated hourly (ISR) rather than per-request — a crawler-facing
 * document doesn't need to be real-time, and this keeps the API/DB reads
 * off the hot path.
 *
 * Coverage note: artists/artworks are derived from a bounded scan of
 * `/v1/search` (the API caps offset pagination at 1000 rows, so this
 * covers up to the first 1000 artworks + their artists — i.e. the active
 * corpus). If the catalogue outgrows that, add a dedicated slug-list
 * endpoint and page it here. Public collections have no list endpoint
 * yet, so `/collections/[id]` pages are intentionally omitted.
 */

const BASE_URL = "https://wander.gallery";

// Page size + page cap for the artwork scan. 10 × 100 = 1000, matching
// the API's MAX_CURSOR_OFFSET. Kept as consts so the bound is explicit.
const SCAN_PAGE_SIZE = 100;
const SCAN_MAX_PAGES = 10;

export const revalidate = 3600; // 1h

export default async function sitemap(): Promise<MetadataRoute.Sitemap> {
  const staticRoutes: MetadataRoute.Sitemap = [
    { url: `${BASE_URL}/`, changeFrequency: "weekly", priority: 1.0 },
    { url: `${BASE_URL}/neighborhoods`, changeFrequency: "weekly", priority: 0.8 },
    { url: `${BASE_URL}/for-artists`, changeFrequency: "monthly", priority: 0.7 },
    { url: `${BASE_URL}/about`, changeFrequency: "monthly", priority: 0.5 },
  ];

  // Neighborhoods — clean public list endpoint.
  let neighborhoodRoutes: MetadataRoute.Sitemap = [];
  try {
    const neighborhoods = await listNeighborhoods({ cache: "no-store" });
    neighborhoodRoutes = neighborhoods.items.map((n) => ({
      url: `${BASE_URL}/neighborhoods/${n.slug}`,
      changeFrequency: "weekly",
      priority: 0.6,
    }));
  } catch (err) {
    // A partial sitemap beats a 500 — degrade to whatever we have.
    reportError(err, { surface: "sitemap", call: "listNeighborhoods" });
  }

  // Artists + artworks — derived from a bounded search scan. Distinct
  // artist slugs give the portfolio pages; artwork ids give the detail
  // pages. Both are canonical, indexable content routes.
  const artistSlugs = new Set<string>();
  const artworkRoutes: MetadataRoute.Sitemap = [];
  try {
    let cursor: string | undefined;
    for (let page = 0; page < SCAN_MAX_PAGES; page++) {
      const result = await searchArtworks(
        { limit: SCAN_PAGE_SIZE, cursor },
        { cache: "no-store" },
      );
      for (const art of result.items) {
        artistSlugs.add(art.artist_slug);
        artworkRoutes.push({
          url: `${BASE_URL}/artworks/${art.id}`,
          changeFrequency: "monthly",
          priority: 0.5,
        });
      }
      if (!result.next_cursor) break;
      cursor = result.next_cursor;
    }
  } catch (err) {
    reportError(err, { surface: "sitemap", call: "searchArtworks" });
  }

  const artistRoutes: MetadataRoute.Sitemap = [...artistSlugs].map((slug) => ({
    url: `${BASE_URL}/artists/${slug}`,
    changeFrequency: "weekly",
    priority: 0.8,
  }));

  return [
    ...staticRoutes,
    ...neighborhoodRoutes,
    ...artistRoutes,
    ...artworkRoutes,
  ];
}

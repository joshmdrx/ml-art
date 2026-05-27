import Link from "next/link";
import { TopNav } from "@/components/TopNav";
import { ArtworkGrid } from "@/components/ArtworkGrid";
import { FilterBar } from "@/components/FilterBar";
import { searchArtworks, type SearchParams, type Availability } from "@/lib/api";
import { priceParamsFromToken } from "@/lib/filterBar";

/**
 * /search?q=...&location=...&medium=...&price=...&availability=...
 *
 * Pulls results from the Rust API and renders them. The FilterBar
 * component drives URL state for medium / price (as a `price` bucket
 * token) / availability / location; everything stays bookmarkable.
 */

type Search = {
  q?: string;
  location?: string;
  near_lat?: string;
  near_lng?: string;
  near_radius_km?: string;
  sort?: string;
  medium?: string;
  /** Bucket token from `lib/filterBar::PRICE_BUCKETS`. Translated to
   * `price_min`/`price_max` cents before the API call. */
  price?: string;
  price_min?: string;
  price_max?: string;
  availability?: string;
};

export default async function SearchPage({
  searchParams,
}: {
  searchParams: Promise<Search>;
}) {
  const sp = await searchParams;

  // If the URL carries a `price` bucket token, derive the API params from
  // it; otherwise honor raw `price_min`/`price_max` (lets power-users hand-
  // edit URLs to set custom ranges the buckets don't cover).
  const bucketPrice = priceParamsFromToken(sp.price);
  const params: SearchParams = {
    q: sp.q?.trim() || undefined,
    location: sp.location?.trim() || undefined,
    near_lat: sp.near_lat ? Number(sp.near_lat) : undefined,
    near_lng: sp.near_lng ? Number(sp.near_lng) : undefined,
    near_radius_km: sp.near_radius_km ? Number(sp.near_radius_km) : undefined,
    sort: (sp.sort as SearchParams["sort"]) || undefined,
    medium: sp.medium?.trim() || undefined,
    price_min:
      bucketPrice?.price_min ??
      (sp.price_min ? Number(sp.price_min) : undefined),
    price_max:
      bucketPrice?.price_max ??
      (sp.price_max ? Number(sp.price_max) : undefined),
    availability: (sp.availability?.trim() || undefined) as
      | Availability
      | undefined,
    limit: 24,
  };

  let resp;
  let error: string | null = null;
  let embedderEnabled = false;
  try {
    resp = await searchArtworks(params);
    // Best-effort: ask the health endpoint whether vector search is on.
    // If the call fails we just don't surface the hint.
    try {
      const h = await fetch(
        `${process.env.NEXT_PUBLIC_API_BASE_URL ?? "http://localhost:9100"}/v1/health`,
        { cache: "no-store" }
      );
      const j = await h.json();
      embedderEnabled = Boolean(j?.embedder_enabled);
    } catch {
      // ignore — empty-state hint just won't appear
    }
  } catch (e) {
    resp = { items: [], next_cursor: null };
    error = e instanceof Error ? e.message : String(e);
  }

  const summary = describeQuery(params);
  const hasTextQuery = Boolean(params.q);

  return (
    <>
      <TopNav initialQuery={params.q ?? ""} />

      <main className="flex-1 mx-auto w-full max-w-screen-2xl px-6 py-10">
        <div className="mb-3 text-sm text-muted">{summary}</div>

        <FilterBar
          availableFilters={["medium", "price", "availability", "location"]}
          basePath="/search"
        />

        {error && (
          <div className="mb-6 p-4 border border-border bg-surface text-sm">
            <p className="font-medium mb-1">Couldn’t reach the search API.</p>
            <p className="text-muted">
              Is{" "}
              <code className="font-mono">
                {process.env.NEXT_PUBLIC_API_BASE_URL}
              </code>{" "}
              running? Error: <code className="font-mono">{error}</code>
            </p>
          </div>
        )}

        {resp.items.length === 0 && !error ? (
          <EmptyState
            hasTextQuery={hasTextQuery}
            query={params.q}
            embedderEnabled={embedderEnabled}
          />
        ) : (
          <ArtworkGrid items={resp.items} />
        )}
      </main>
    </>
  );
}

function EmptyState({
  hasTextQuery,
  query,
  embedderEnabled,
}: {
  hasTextQuery: boolean;
  query?: string;
  embedderEnabled: boolean;
}) {
  // Example queries we know match in keyword-only mode — these all appear
  // as a `medium` value on seeded demo artworks.
  const examples = [
    "ukiyo",
    "cubism",
    "impressionism",
    "color field",
    "minimal",
    "pop art",
    "baroque",
    "realism",
  ];

  return (
    <div className="py-24 text-center max-w-2xl mx-auto">
      <p className="font-serif text-2xl">No artworks match this search.</p>
      {hasTextQuery && !embedderEnabled && (
        <div className="mt-6 text-sm text-muted leading-relaxed">
          <p>
            Vector search is currently off — the API is matching{" "}
            <code className="font-mono">{query}</code> only against title,
            medium, and description. The demo corpus is labelled by style,
            so terms like <em>blue</em>, <em>moody</em>, or <em>landscape</em>{" "}
            won&apos;t hit anything until <code>JINA_API_KEY</code> is set
            and CLIP-style semantic search runs against the artwork images.
          </p>
          <p className="mt-4">
            Try one of these instead:
          </p>
          <ul className="mt-3 flex flex-wrap gap-2 justify-center">
            {examples.map((q) => (
              <li key={q}>
                <a
                  href={`/search?q=${encodeURIComponent(q)}`}
                  className="px-3 py-1 border border-border bg-surface hover:bg-background"
                >
                  {q}
                </a>
              </li>
            ))}
          </ul>
        </div>
      )}
      {hasTextQuery && embedderEnabled && (
        <p className="mt-6 text-sm text-muted">
          Try fewer filters or a different query.
        </p>
      )}
      {!hasTextQuery && (
        <p className="mt-6 text-sm text-muted">
          Try removing some filters, or{" "}
          <Link href="/" className="underline hover:no-underline">
            head back to the homepage
          </Link>
          .
        </p>
      )}
    </div>
  );
}

function describeQuery(p: SearchParams): string {
  const parts: string[] = [];
  if (p.q) parts.push(`“${p.q}”`);
  if (p.location) parts.push(`in ${p.location}`);
  if (p.near_lat && p.near_lng) {
    parts.push(
      `within ${p.near_radius_km ?? 50}km of ${p.near_lat.toFixed(2)},${p.near_lng.toFixed(2)}`
    );
  }
  if (parts.length === 0) return "Showing all artworks";
  return `Results for ${parts.join(" ")}`;
}

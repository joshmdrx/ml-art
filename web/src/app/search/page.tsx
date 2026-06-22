import Link from "next/link";
import { TopNav } from "@/components/TopNav";
import { ArtworkGrid } from "@/components/ArtworkGrid";
import { FilterBar } from "@/components/FilterBar";
import { ModifierBar } from "@/components/ModifierBar";
import { SearchSplitView } from "@/components/SearchSplitView";
import {
  getArtwork,
  listMapCities,
  listSearchModifiers,
  searchArtworks,
  searchMap,
  type Availability,
  type ArtworkFull,
  type CityPivot,
  type MapPin,
  type SearchParams,
} from "@/lib/api";
import { priceParamsFromToken } from "@/lib/filterBar";
import { reportError, toUserMessage } from "@/lib/reportError";

/** Page size for the grid query. Surfaced as a named const because
 * the disconnect explainer needs to know whether the result set is
 * "exactly N" or "N+ (we capped)" — those read very differently to a
 * user staring at zero map pins. */
const GRID_PAGE_LIMIT = 24;

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
  /** Visual-search anchor — set after a successful upload. T-010 Phase D. */
  image_upload_id?: string;
  /** Visual-search anchor sourced from an existing platform artwork
   * (no upload roundtrip). Set by the "Find visually similar →" CTA
   * on /artworks/[id]. Server reads the artwork's vector out of
   * `artwork_embeddings` directly. Modifiers compose normally. */
  seed_artwork_id?: string;
  /** Comma-separated modifier names. Server rejects unknown values. */
  modifiers?: string;
  /** Surfaced by `actions/visualSearch::uploadAndStartVisualSearch`
   * when the upload itself failed. */
  upload_error?: string;
  /** Map mode toggle (T-038 G5). `?map=1` swaps the grid for a Mapbox
   * GL JS view of venues matching the active filters. */
  map?: string;
  /** Mapbox bounds when in map mode, "west,south,east,north". The
   * client component owns mutating this as the user pans/zooms. */
  bbox?: string;
  /** Pin the map down to a single artist's locations (T-041). Set by
   * the "See on map" CTA on `/artists/[slug]`. */
  artist?: string;
  /** Cumulative page count (1..MAX_PAGES). Each Load More click bumps
   * this by 1 via router.push; the server fetches `pages` cursor-chained
   * pages and returns them concatenated. URL-driven so back-nav,
   * refresh, and shared links all reproduce the same view. */
  pages?: string;
  /** Currently-focused artwork (set by sidebar card click via
   * replaceState). On mount, restores the popup + scrolls the card
   * into view — "back-nav from /artists/[slug] lands me back on the
   * same selected work." */
  focus?: string;
};

/** Hard cap on `?pages` to bound server fan-out per render. Each page
 * is a sequential roundtrip to `/v1/search`; 10 pages * 24 items =
 * 240 results, which is the practical horizon for an exploration
 * session before the user should refine filters. */
const MAX_PAGES = 10;

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
    image_upload_id: sp.image_upload_id?.trim() || undefined,
    seed_artwork_id: sp.seed_artwork_id?.trim() || undefined,
    modifiers: sp.modifiers?.trim() || undefined,
    limit: GRID_PAGE_LIMIT,
  };

  // Cumulative pages (URL-driven Load More). Sequentially chase the
  // cursor up to `pages` times — sequential not parallel because the
  // cursor is opaque (callers shouldn't peek inside; today it's an
  // offset, tomorrow keyset, so we follow the chain). For v1 scale
  // (<= 10 pages * ~150ms per call) the latency is acceptable.
  const pagesRequested = Math.max(
    1,
    Math.min(MAX_PAGES, parseInt(sp.pages ?? "1", 10) || 1),
  );
  let resp: { items: import("@/lib/api").ArtworkSummary[]; next_cursor: string | null } = {
    items: [],
    next_cursor: null,
  };
  let error: string | null = null;
  let embedderEnabled = false;
  try {
    let cursor: string | undefined;
    for (let p = 0; p < pagesRequested; p++) {
      const page = await searchArtworks({ ...params, cursor });
      resp = {
        items: [...resp.items, ...page.items],
        next_cursor: page.next_cursor ?? null,
      };
      if (!page.next_cursor) break; // ran out of pages before pages cap
      cursor = page.next_cursor;
    }
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
    error = toUserMessage(
      e,
      "Couldn't reach the search API. Try again in a moment.",
      { surface: "search-page", call: "searchArtworks" },
    );
  }

  // Map mode (T-038 G5). Server-fetches the first page of pins so
  // the initial render has data even before the client's Mapbox
  // module loads; the client component takes over for pan/zoom
  // refetches.
  //
  // "Map = view of grid result": when the search has any filter
  // (q / medium / location), we forward the distinct artist_ids from
  // the grid response into the map fetch. That way the map mirrors
  // the same RRF/vector retrieval the grid ran — no second embed,
  // no diverging result sets. The plain "Explore the map" path
  // (no filter) skips this so the cold-start view still shows every
  // geocoded artist.
  const mapMode = sp.map === "1";
  let mapPins: MapPin[] = [];
  let mapError: string | null = null;
  let mapCities: CityPivot[] = [];
  const hasGridFilter =
    Boolean(params.q) ||
    Boolean(params.medium) ||
    Boolean(params.location);
  const gridArtistIds: string[] = hasGridFilter
    ? Array.from(new Set(resp.items.map((a) => a.artist_id)))
    : [];
  const artistIdsParam =
    gridArtistIds.length > 0 ? gridArtistIds.join(",") : undefined;
  if (mapMode) {
    // Fetch pins + city pivots in parallel — both small reads, both
    // needed for first paint.
    const [pinsResult, citiesResult] = await Promise.allSettled([
      searchMap({
        // When artist_ids is set, the API ignores q/medium/location
        // for filtering (the upstream grid has already applied them
        // to derive the id set). We still forward `artist` (single
        // slug) because that composes orthogonally.
        //
        // `bbox` is *dropped* in filtered mode: when artist_ids is
        // set, bbox would clip the pin list to the current viewport,
        // hiding pins for artists whose venue is offscreen — which
        // breaks the card-to-pin click flow (clicking a card flies
        // to nowhere because `pins.find(slug)` returns nothing). The
        // invariant we want is "map has every pin for every artist
        // in the grid result; Mapbox decides what's visible." This
        // mirrors the client-side `refetchOnPan: !hasActiveFilter`
        // logic in `useMapBboxSync`.
        artist_ids: artistIdsParam,
        q: artistIdsParam ? undefined : params.q,
        medium: artistIdsParam ? undefined : params.medium,
        location: artistIdsParam ? undefined : params.location,
        artist: sp.artist?.trim() || undefined,
        bbox: artistIdsParam ? undefined : sp.bbox?.trim() || undefined,
      }),
      listMapCities({
        // Mirror the same filter selection as the pins query so the
        // city strip and the map agree on which artists are in play.
        q: artistIdsParam ? undefined : params.q,
        medium: artistIdsParam ? undefined : params.medium,
        artist_ids: artistIdsParam ? gridArtistIds : undefined,
      }),
    ]);
    if (pinsResult.status === "fulfilled") {
      mapPins = pinsResult.value;
    } else {
      reportError(pinsResult.reason, { surface: "search-map-initial" });
      mapError =
        pinsResult.reason instanceof Error
          ? pinsResult.reason.message
          : String(pinsResult.reason);
    }
    if (citiesResult.status === "fulfilled") {
      mapCities = citiesResult.value;
    } else {
      // City pivots are a nice-to-have; failure shouldn't blow up the map.
      reportError(citiesResult.reason, { surface: "search-map-cities" });
    }
  }

  // Fetch the modifier registry only when we'll render the bar — i.e.
  // there's any visual anchor (uploaded OR seeded from a platform
  // artwork). Saves a call in the common no-image case.
  const visualMode =
    Boolean(params.image_upload_id) || Boolean(params.seed_artwork_id);
  const modifiers = visualMode
    ? await listSearchModifiers().catch((e) => {
        reportError(e, { surface: "search-modifiers" });
        return [];
      })
    : [];

  // When seeding from a platform artwork, fetch its summary for the
  // anchor strip — gives the user a real thumbnail + title to confirm
  // they're searching from the right work. Failure is non-fatal; the
  // anchor strip falls back to a minimal display.
  const seedArtwork = params.seed_artwork_id
    ? await getArtwork(params.seed_artwork_id).catch((e) => {
        reportError(e, {
          surface: "search-seed-artwork",
          id: params.seed_artwork_id,
        });
        return null;
      })
    : null;

  const summary = describeQuery(params);
  const hasTextQuery = Boolean(params.q);

  return (
    <>
      <TopNav initialQuery={params.q ?? ""} />

      <main className="flex-1 mx-auto w-full max-w-screen-2xl px-6 py-10">
        <div className="mb-3 text-sm text-muted">{summary}</div>

        {sp.upload_error && (
          <div className="mb-6 p-4 border border-border bg-surface text-sm">
            <p className="font-medium mb-1">Image upload failed.</p>
            <p className="text-muted">
              Try a different image (JPG / PNG / WebP, under 10 MB).
            </p>
          </div>
        )}

        {visualMode && params.image_upload_id && (
          <VisualAnchor uploadId={params.image_upload_id} />
        )}

        {visualMode && params.seed_artwork_id && (
          <SeedAnchor
            artworkId={params.seed_artwork_id}
            artwork={seedArtwork}
          />
        )}

        {visualMode && modifiers.length > 0 && (
          <ModifierBar modifiers={modifiers} />
        )}

        <FilterBar
          availableFilters={["medium", "price", "size", "availability", "location"]}
          basePath="/search"
        />

        <ViewToggle mapMode={mapMode} searchParams={sp} />

        {error && (
          <div className="mb-6 p-4 border border-border bg-surface text-sm">
            <p>{error}</p>
          </div>
        )}

        {mapMode ? (
          <SearchSplitView
            items={resp.items}
            initialNextCursor={resp.next_cursor ?? null}
            emptyState={
              !error ? (
                <EmptyState
                  hasTextQuery={hasTextQuery}
                  query={params.q}
                  embedderEnabled={embedderEnabled}
                />
              ) : null
            }
            mapBlockProps={{
              pins: mapPins,
              filters: {
                // Same logic as the server-side fetch above: when
                // artist_ids drives the result set, q/medium/location
                // are upstream-applied and would only confuse the
                // refetch on pan/zoom.
                artist_ids: artistIdsParam,
                q: artistIdsParam ? undefined : params.q,
                medium: artistIdsParam ? undefined : params.medium,
                location: artistIdsParam ? undefined : params.location,
                artist: sp.artist?.trim() || undefined,
              },
              artistSlug: sp.artist?.trim() || undefined,
              cities: mapCities,
              searchParams: sp,
              error: mapError,
            }}
          />
        ) : resp.items.length === 0 && !error ? (
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

/**
 * Grid / Map view toggle. Both tabs preserve the rest of the URL (q,
 * medium, etc.); only the `map` param flips. `bbox` is dropped when
 * we leave map mode so a stale bbox doesn't constrain the grid query.
 */
function ViewToggle({
  mapMode,
  searchParams,
}: {
  mapMode: boolean;
  searchParams: Search;
}) {
  function hrefFor(map: boolean): string {
    const usp = new URLSearchParams();
    for (const [k, v] of Object.entries(searchParams)) {
      if (k === "map" || k === "bbox") continue;
      if (typeof v === "string" && v.length > 0) usp.set(k, v);
    }
    if (map) usp.set("map", "1");
    const qs = usp.toString();
    return `/search${qs ? `?${qs}` : ""}`;
  }

  return (
    <div className="my-4 flex items-center gap-3">
      <div
        role="tablist"
        aria-label="Result view"
        className="inline-flex border border-border"
      >
      {/* Labels lean into "two lenses on different data" rather than
          "same data, two views" — the map shows where to see the
          artists in person, which is a smaller set than the artwork
          results (only artists with geocoded locations appear). URL
          token stays `?map=1` for backward compatibility. */}
      <Link
        href={hrefFor(false)}
        role="tab"
        aria-selected={!mapMode}
        className={`px-3 py-1 text-sm ${
          !mapMode ? "bg-fg text-bg" : "hover:bg-surface"
        }`}
      >
        Works
      </Link>
      <Link
        href={hrefFor(true)}
        role="tab"
        aria-selected={mapMode}
        className={`px-3 py-1 text-sm border-l border-border ${
          mapMode ? "bg-fg text-bg" : "hover:bg-surface"
        }`}
      >
        Where to see them
      </Link>
      </div>
      {/* Near-me lives inside the map's control stack now (T-043
          revisited) — see `SearchMap`. Keeps the page toolbar clean
          and matches the Google-Maps / Mapbox convention. */}
    </div>
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
  if (p.image_upload_id) parts.push("your uploaded image");
  if (p.seed_artwork_id) parts.push("a platform artwork");
  if (p.q) parts.push(`“${p.q}”`);
  if (p.modifiers) parts.push(`modified by ${p.modifiers.replace(/_/g, " ")}`);
  if (p.location) parts.push(`in ${p.location}`);
  if (p.near_lat && p.near_lng) {
    parts.push(
      `within ${p.near_radius_km ?? 50}km of ${p.near_lat.toFixed(2)},${p.near_lng.toFixed(2)}`
    );
  }
  if (parts.length === 0) return "Showing all artworks";
  return `Results for ${parts.join(" ")}`;
}

/** Renders the uploaded image at the top of the search results with a
 * "clear" link that drops the visual anchor (back to plain search).
 * The thumbnail URL is reconstructed from the configured public prefix
 * + the known `s3_key` shape (`uploads/<uuid>.<ext>`) — we don't have
 * the s3_key on hand, only the upload_id, so we'd need an API roundtrip
 * to fetch it. For v0 we display the upload_id text. Real thumbnail
 * preview lands when we add `GET /v1/uploads/:id` (T-010 Phase D+). */
function VisualAnchor({ uploadId }: { uploadId: string }) {
  return (
    <div className="mb-4 p-3 flex items-center gap-3 border border-border bg-surface text-sm">
      <span className="text-muted">Searching by image</span>
      <code className="font-mono text-xs text-muted">{uploadId.slice(0, 8)}…</code>
      <Link
        href="/search"
        className="ml-auto text-xs underline underline-offset-2 hover:text-foreground"
      >
        Clear image
      </Link>
    </div>
  );
}

/**
 * Anchor strip for the seed-from-platform-artwork visual search
 * (T-046 ish — added 2026-06-09). Unlike `VisualAnchor`, we have
 * the artwork details server-side so we render a real thumbnail +
 * title + link back to the source artwork. Falls back to a minimal
 * "seeded by X" line when the artwork fetch failed (the search
 * itself still works because the embedding lookup is independent).
 */
function SeedAnchor({
  artworkId,
  artwork,
}: {
  artworkId: string;
  artwork: ArtworkFull | null;
}) {
  const primary =
    artwork?.images?.find((i) => i.is_primary) ?? artwork?.images?.[0];
  return (
    <div className="mb-4 p-3 flex items-center gap-3 border border-border bg-surface text-sm">
      {primary ? (
        // eslint-disable-next-line @next/next/no-img-element
        <img
          src={primary.url}
          alt=""
          className="w-12 h-12 object-cover bg-background flex-shrink-0"
        />
      ) : null}
      <span className="text-muted">Visually similar to</span>
      {artwork ? (
        <Link
          href={`/artworks/${artworkId}`}
          className="font-serif hover:underline"
        >
          {artwork.title ?? "this work"}
          <span className="text-muted font-sans text-xs ml-1">
            by {artwork.artist.display_name}
          </span>
        </Link>
      ) : (
        <code className="font-mono text-xs text-muted">
          {artworkId.slice(0, 8)}…
        </code>
      )}
      <Link
        href="/search"
        className="ml-auto text-xs underline underline-offset-2 hover:text-foreground"
      >
        Clear
      </Link>
    </div>
  );
}

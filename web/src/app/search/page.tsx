import Link from "next/link";
import { TopNav } from "@/components/TopNav";
import { ArtworkGrid } from "@/components/ArtworkGrid";
import { FilterBar } from "@/components/FilterBar";
import { ModifierBar } from "@/components/ModifierBar";
import { CityPivotStrip } from "@/components/CityPivotStrip";
import { SearchMap } from "@/components/SearchMap";
import {
  listMapCities,
  listSearchModifiers,
  searchArtworks,
  searchMap,
  type Availability,
  type CityPivot,
  type MapPin,
  type SearchParams,
} from "@/lib/api";
import { priceParamsFromToken } from "@/lib/filterBar";
import { reportError } from "@/lib/reportError";

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
    image_upload_id: sp.image_upload_id?.trim() || undefined,
    modifiers: sp.modifiers?.trim() || undefined,
    limit: GRID_PAGE_LIMIT,
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
        // slug) + `bbox` because those compose orthogonally.
        artist_ids: artistIdsParam,
        q: artistIdsParam ? undefined : params.q,
        medium: artistIdsParam ? undefined : params.medium,
        location: artistIdsParam ? undefined : params.location,
        artist: sp.artist?.trim() || undefined,
        bbox: sp.bbox?.trim() || undefined,
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
  // there's an image anchor. Saves a call in the common no-image case.
  const visualMode = Boolean(params.image_upload_id);
  const modifiers = visualMode
    ? await listSearchModifiers().catch((e) => {
        reportError(e, { surface: "search-modifiers" });
        return [];
      })
    : [];

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
              <code className="font-mono">{sp.upload_error}</code>
            </p>
          </div>
        )}

        {visualMode && params.image_upload_id && (
          <VisualAnchor uploadId={params.image_upload_id} />
        )}

        {visualMode && modifiers.length > 0 && (
          <ModifierBar modifiers={modifiers} />
        )}

        <FilterBar
          availableFilters={["medium", "price", "availability", "location"]}
          basePath="/search"
        />

        <ViewToggle mapMode={mapMode} searchParams={sp} />

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

        {mapMode ? (
          /* L1 split view (T-045): grid moves to a side panel, map
             takes the rest. Stacked on mobile (map first, then
             cards), two-column on lg+ (cards left, map right).
             Hover/click sync between the two panes comes in L2/L3 —
             this slice is layout-only.

             Order: map first in source order, but visually flipped
             to "cards left, map right" on lg+ via `lg:order-*`. This
             keeps mobile users scrolling map → cards (the map is
             the cue for why they tapped "Where to see them"). */
          <div className="flex flex-col gap-6 lg:grid lg:grid-cols-[minmax(0,2fr)_minmax(0,3fr)] lg:gap-6 lg:items-start">
            <aside
              aria-label="Search results"
              className="order-2 lg:order-1 lg:sticky lg:top-4 lg:max-h-[640px] lg:overflow-y-auto lg:pr-2"
            >
              {resp.items.length > 0 ? (
                <>
                  <p className="mb-3 text-xs uppercase tracking-wider text-muted">
                    {resp.items.length}
                    {resp.items.length >= GRID_PAGE_LIMIT ? "+" : ""} work
                    {resp.items.length === 1 ? "" : "s"}
                  </p>
                  <ArtworkGrid items={resp.items} density="compact" />
                </>
              ) : !error ? (
                <EmptyState
                  hasTextQuery={hasTextQuery}
                  query={params.q}
                  embedderEnabled={embedderEnabled}
                />
              ) : null}
            </aside>
            <div className="order-1 lg:order-2">
              <SearchMapBlock
                pins={mapPins}
                filters={{
                  // Same logic as the server-side fetch above: when
                  // artist_ids drives the result set, q/medium/location
                  // are upstream-applied and would only confuse the
                  // refetch on pan/zoom.
                  artist_ids: artistIdsParam,
                  q: artistIdsParam ? undefined : params.q,
                  medium: artistIdsParam ? undefined : params.medium,
                  location: artistIdsParam ? undefined : params.location,
                  artist: sp.artist?.trim() || undefined,
                }}
                artistSlug={sp.artist?.trim() || undefined}
                cities={mapCities}
                searchParams={sp}
                error={mapError}
                gridResultCount={resp.items.length}
                gridHitLimit={resp.items.length >= GRID_PAGE_LIMIT}
                hasActiveFilter={
                  Boolean(params.q) ||
                  Boolean(params.medium) ||
                  Boolean(params.location)
                }
              />
            </div>
          </div>
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

function SearchMapBlock({
  pins,
  filters,
  artistSlug,
  cities,
  searchParams,
  error,
  gridResultCount,
  gridHitLimit,
  hasActiveFilter,
}: {
  pins: MapPin[];
  filters: {
    q?: string;
    medium?: string;
    location?: string;
    artist?: string;
    /** Comma-joined artist UUIDs — "map = view of grid result". */
    artist_ids?: string;
  };
  /** Same as `filters.artist` but lifted out so the scoping pill can
   * derive the "Clear filter" link without duplicating the prop. */
  artistSlug?: string;
  /** Top-cities pivot (T-042). Empty array when nothing's geocoded
   * yet (cold-start). */
  cities: CityPivot[];
  /** Full URL search params so the "Clear filter" link can preserve
   * every other filter the artist had set. */
  searchParams: Search;
  error: string | null;
  /** How many works the parallel grid query returned. Used for the
   * disconnect explainer: if the grid has results but the map is
   * empty, we tell the user why instead of leaving them staring at
   * a silent blank globe. */
  gridResultCount: number;
  /** True when the grid response was capped at `GRID_PAGE_LIMIT`,
   * meaning the real number of matching works is unknown. Lets the
   * disconnect explainer render "N+ works" instead of lying with a
   * precise number. */
  gridHitLimit: boolean;
  /** Whether any text/medium/location filter was applied. The
   * disconnect explainer only fires when the user actually
   * searched — otherwise we'd show it on cold-start with no pins,
   * which is misleading (the right copy is "no venues yet"). */
  hasActiveFilter: boolean;
}) {
  if (error) {
    return (
      <div className="mb-6 p-4 border border-border bg-surface text-sm">
        <p className="font-medium mb-1">Couldn’t load map results.</p>
        <p className="text-muted">
          <code className="font-mono">{error}</code>
        </p>
      </div>
    );
  }

  // Build a "clear artist filter" href that keeps every other param.
  function clearArtistHref(): string {
    const usp = new URLSearchParams();
    for (const [k, v] of Object.entries(searchParams)) {
      if (k === "artist") continue;
      if (typeof v === "string" && v.length > 0) usp.set(k, v);
    }
    return `/search?${usp.toString()}`;
  }

  // Pick a display name for the pill. We don't have the artist's
  // display name on this surface — only the slug — so we de-kebab
  // ("josh-matthews" → "Josh Matthews"). Close-enough for a chip;
  // when the user clicks any pin they see the real display name.
  function prettifySlug(slug: string): string {
    return slug
      .split("-")
      .map((p) => (p ? p[0].toUpperCase() + p.slice(1) : p))
      .join(" ");
  }

  return (
    <>
      {artistSlug && (
        <div
          className="mb-4 inline-flex items-center gap-3 text-sm bg-surface border border-border px-3 py-1.5"
          role="status"
        >
          <span>
            Showing where to see <strong>{prettifySlug(artistSlug)}</strong>
          </span>
          <Link
            href={clearArtistHref()}
            className="text-muted underline hover:text-foreground"
          >
            Clear filter
          </Link>
        </div>
      )}
      {/* Disconnect explainer: grid has matches but the map doesn't.
          Two distinct queries (Works = RRF over artworks; Where to
          see them = artist_locations of matching artists), so a
          search that lands in the grid can still leave the map empty.
          Without this, users see "Map" silently render zero pins and
          assume it's broken. */}
      {pins.length === 0 &&
        gridResultCount > 0 &&
        hasActiveFilter &&
        !artistSlug && (
          <div
            role="status"
            className="mb-4 border border-border bg-surface px-4 py-3 text-sm"
          >
            <p className="font-medium">No public venues for these results.</p>
            <p className="mt-1 text-muted">
              {/* Single-string template — splitting across JSX text
                  nodes was eating the space between "match" and
                  "this" because of how React collapses whitespace
                  around `{...}` expressions on broken lines. */}
              {`${gridResultCount}${gridHitLimit ? "+" : ""} ${
                gridResultCount === 1 ? "work matches" : "works match"
              } this search, but the artists haven’t shared a public studio or gallery location yet.`}{" "}
              <Link
                href={clearMapHref(searchParams)}
                className="underline underline-offset-2 hover:text-foreground"
              >
                Back to Works →
              </Link>
            </p>
          </div>
        )}
      <CityPivotStrip cities={cities} />
      <SearchMap initial={pins} filters={filters} />
    </>
  );
}

/** Build the "go back to the works grid" href, preserving every URL
 * param except `map` + `bbox` (those are map-mode only). Defined at
 * module scope so the SearchMapBlock JSX stays focused. */
function clearMapHref(searchParams: Search): string {
  const usp = new URLSearchParams();
  for (const [k, v] of Object.entries(searchParams)) {
    if (k === "map" || k === "bbox") continue;
    if (typeof v === "string" && v.length > 0) usp.set(k, v);
  }
  const qs = usp.toString();
  return `/search${qs ? `?${qs}` : ""}`;
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

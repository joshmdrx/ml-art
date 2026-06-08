/**
 * Map column for `/search?map=1` (T-038 G5, T-045 L1+L2).
 *
 * Wraps `<SearchMap>` with the surrounding chrome:
 *   - `<FilterPill>` for the artist filter (T-041) — artist isn't in
 *     the FilterBar's facet list so this pill is its only "clear"
 *     affordance. Location is *not* mirrored here because the
 *     FilterBar already shows a "Location: X ×" facet.
 *   - the disconnect explainer (when grid hit but map didn't)
 *   - the CityPivotStrip (cold-start + city-jump affordance, hides
 *     itself once a location filter is active)
 *
 * Receives `highlightedArtistSlug` from the parent SplitView so
 * the card hover lifts a feature-state on the matching pin(s).
 *
 * Lives in its own file (rather than inline in `/search/page.tsx`)
 * because the SearchSplitView client component needs to render it
 * directly with state from its own scope.
 */

import Link from "next/link";

import type { CityPivot, MapPin } from "@/lib/api";
import { searchHref } from "@/lib/searchMap/url";

import { CityPivotStrip } from "./CityPivotStrip";
import { SearchMap } from "./SearchMap";
import { FilterPill } from "./SearchMap/FilterPill";
import type { FocusSignal } from "./SearchMap/useFocusArtist";

/** Subset of the page's URL search params that the disconnect-explainer
 * "Back to Works →" link wants to preserve. Mirrors the shape used
 * by `/search/page.tsx`'s top-level `Search` type, but kept loose
 * here so this component doesn't need to import the page-internal
 * type. */
type SearchParamsLike = Record<string, string | string[] | undefined>;

export interface SearchMapBlockProps {
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
  /** Full URL search params so "Clear filter" / "Back to Works" can
   * preserve every other filter the user had set. */
  searchParams: SearchParamsLike;
  error: string | null;
  /** How many works the parallel grid query returned. Used by the
   * disconnect explainer. */
  gridResultCount: number;
  /** True when the grid response was capped at the page limit. */
  gridHitLimit: boolean;
  /** Whether any text/medium/location filter was applied. The
   * disconnect explainer only fires when a filter is active. */
  hasActiveFilter: boolean;
  /** L2 hover-sync: the artist whose pin(s) should render in
   * `feature-state.highlighted = true`. Threaded through to
   * `<SearchMap>`. */
  highlightedArtistSlug?: string | null;
  /** L3 click-sync: signals "focus this artist on the map".
   * Threaded through to `<SearchMap>` → `useFocusArtist`. */
  focusSignal?: FocusSignal | null;
}

export function SearchMapBlock({
  pins,
  filters,
  artistSlug,
  cities,
  searchParams,
  error,
  gridResultCount,
  gridHitLimit,
  hasActiveFilter,
  highlightedArtistSlug,
  focusSignal,
}: SearchMapBlockProps) {
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

  // Read the location filter from the URL, *not* from `filters.location`.
  // `filters` is the API call shape: page.tsx blanks `location` out when
  // `artist_ids` is in play (the upstream grid has already applied it).
  // We need the URL truth here for the disconnect-explainer's "is a
  // location filter active?" gate — otherwise the explainer fires when
  // the user picked a city that has no public venues, which is
  // confusing because *they* chose the city.
  //
  // No location *pill* is rendered here: the FilterBar already shows
  // a "Location: X ×" facet pill, so adding another would duplicate
  // the affordance. The camera-refit-on-clear behaviour is owned by
  // `useFitToInitialPins`, which doesn't care who dropped the param.
  const locationFilter = (() => {
    const raw = searchParams.location;
    const v = Array.isArray(raw) ? raw[0] : raw;
    return typeof v === "string" && v.length > 0 ? v : undefined;
  })();

  function prettifySlug(slug: string): string {
    return slug
      .split("-")
      .map((p) => (p ? p[0].toUpperCase() + p.slice(1) : p))
      .join(" ");
  }

  return (
    <>
      {artistSlug && (
        <FilterPill
          label="Showing where to see"
          value={prettifySlug(artistSlug)}
          clearHref={searchHref(searchParams, { drop: ["artist"] })}
        />
      )}
      {pins.length === 0 &&
        gridResultCount > 0 &&
        hasActiveFilter &&
        !artistSlug &&
        !locationFilter && (
          <div
            role="status"
            className="mb-4 border border-border bg-surface px-4 py-3 text-sm"
          >
            <p className="font-medium">No public venues for these results.</p>
            <p className="mt-1 text-muted">
              {`${gridResultCount}${gridHitLimit ? "+" : ""} ${
                gridResultCount === 1 ? "work matches" : "works match"
              } this search, but the artists haven’t shared a public studio or gallery location yet.`}{" "}
              <Link
                href={searchHref(searchParams, { drop: ["map", "bbox"] })}
                className="underline underline-offset-2 hover:text-foreground"
              >
                Back to Works →
              </Link>
            </p>
          </div>
        )}
      <CityPivotStrip cities={cities} />
      <SearchMap
        initial={pins}
        filters={filters}
        highlightedArtistSlug={highlightedArtistSlug ?? null}
        focusSignal={focusSignal ?? null}
      />
    </>
  );
}

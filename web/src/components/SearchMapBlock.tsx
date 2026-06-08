/**
 * Map column for `/search?map=1` (T-038 G5, T-045 L1–L4).
 *
 * Wraps `<SearchMap>` with the surrounding chrome:
 *   - `<FilterPill>` for the artist filter (T-041) — artist isn't in
 *     the FilterBar's facet list so this pill is its only "clear"
 *     affordance. Location is *not* mirrored here because the
 *     FilterBar already shows a "Location: X ×" facet.
 *   - the CityPivotStrip (cold-start + city-jump affordance, hides
 *     itself once a location filter is active)
 *
 * The "0 venues for these results" disconnect explainer that used
 * to live here has been retired (T-045 L4): the SidePanel's new
 * "N of M mapped" caption carries the same information without
 * being a hostile blocking message.
 *
 * Receives `highlightedArtistSlug` from the parent SplitView so
 * the card hover lifts a feature-state on the matching pin(s).
 *
 * Lives in its own file (rather than inline in `/search/page.tsx`)
 * because the SearchSplitView client component needs to render it
 * directly with state from its own scope.
 */

import type { CityPivot, MapPin } from "@/lib/api";
import { searchHref } from "@/lib/searchMap/url";

import { CityPivotStrip } from "./CityPivotStrip";
import { SearchMap } from "./SearchMap";
import { FilterPill } from "./SearchMap/FilterPill";
import type { FocusSignal } from "./SearchMap/useFocusArtist";

/** Subset of the page's URL search params that "Clear filter" links
 * want to preserve. Mirrors the shape used by `/search/page.tsx`'s
 * top-level `Search` type, but kept loose here so this component
 * doesn't need to import the page-internal type. */
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
  /** Full URL search params so "Clear filter" can preserve every
   * other filter the user had set. */
  searchParams: SearchParamsLike;
  error: string | null;
  /** L2 hover-sync: the artist whose pin(s) should render in
   * `feature-state.highlighted = true`. Threaded through to
   * `<SearchMap>`. */
  highlightedArtistSlug?: string | null;
  /** L3 click-sync: signals "focus this artist on the map".
   * Threaded through to `<SearchMap>` → `useFocusArtist`. */
  focusSignal?: FocusSignal | null;
  /** L4: mirror the live pin set back up to the SplitView so the
   * SidePanel can compute "N of M mapped" + (eventually) pan-aware
   * sort. Threaded through to `<SearchMap>`. */
  onPinsChanged?: (pins: MapPin[]) => void;
}

export function SearchMapBlock({
  pins,
  filters,
  artistSlug,
  cities,
  searchParams,
  error,
  highlightedArtistSlug,
  focusSignal,
  onPinsChanged,
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
      <CityPivotStrip cities={cities} />
      <SearchMap
        initial={pins}
        filters={filters}
        highlightedArtistSlug={highlightedArtistSlug ?? null}
        focusSignal={focusSignal ?? null}
        onPinsChanged={onPinsChanged}
      />
    </>
  );
}

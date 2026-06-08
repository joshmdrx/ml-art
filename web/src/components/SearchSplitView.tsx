"use client";

/**
 * Two-column shell for `/search?map=1` (T-045 L1–L4).
 *
 * Owns three pieces of cross-pane state:
 *
 *   - `highlightedArtistSlug` (L2) — side-panel card hover lifts the
 *     artist's pin(s) into `feature-state.highlighted = true`.
 *
 *   - `focusSignal` (L3) — `{ artistSlug, tick }`. Card click drives
 *     the map to flyTo + open the pin's popup. The `tick` increment
 *     re-fires the effect on repeat clicks of the same card.
 *
 *   - `visiblePins` (L4) — live pin set surfaced up from `<SearchMap>`
 *     via the `onPinsChanged` callback. Lets the SidePanel render the
 *     "N of M mapped" caption (and, later, sort cards pan-aware).
 *     Synced via React's "shadow prop with prev-prop slot" derived-
 *     state pattern so a server-side push (filter change) lands in
 *     the same render rather than the one after.
 *
 * Source-of-truth pattern: both panes derive their visual state from
 * the same lifted values. Neither pane mutates state from an event
 * it originated, so no ping-pong loops.
 */

import { useMemo, useState } from "react";

import type { ArtworkSummary, MapPin } from "@/lib/api";
import { searchHref } from "@/lib/searchMap/url";

import {
  SearchMapBlock,
  type SearchMapBlockProps,
} from "./SearchMapBlock";
import type { FocusSignal } from "./SearchMap/useFocusArtist";
import { SearchSidePanel, mappedCountLabel } from "./SearchSidePanel";

interface Props {
  items: ArtworkSummary[];
  /** Rendered into the side panel when `items.length === 0` (and
   * there's no error). Passed in by the page so the EmptyState
   * stays a single source of copy across grid + split views. */
  emptyState: React.ReactNode;
  /** Page size used to render the "M+ works" caption when truncated. */
  pageLimit: number;
  mapBlockProps: Omit<
    SearchMapBlockProps,
    "highlightedArtistSlug" | "focusSignal" | "onPinsChanged"
  >;
}

export function SearchSplitView({
  items,
  emptyState,
  pageLimit,
  mapBlockProps,
}: Props) {
  const [highlightedArtistSlug, setHighlightedArtistSlug] = useState<
    string | null
  >(null);

  // Mobile bottom-sheet expansion (L4c). Defaults to collapsed so the
  // map is fully visible on first paint — the whole point of map
  // mode on mobile is *the map*. Desktop ignores this state.
  const [sheetExpanded, setSheetExpanded] = useState(false);

  const [focusSignal, setFocusSignal] = useState<FocusSignal | null>(null);
  const onFocusArtist = (slug: string) => {
    setFocusSignal((prev) => ({
      artistSlug: slug,
      tick: (prev?.tick ?? 0) + 1,
    }));
  };

  // Live pin set, mirrored from SearchMap. Initialize from the
  // server's payload + sync via the derived-state pattern so a
  // navigation that pushes new initial pins lands immediately
  // (the effect-driven callback fires one render later, which
  // would flash a stale mapped-count).
  const [prevServerPins, setPrevServerPins] = useState(mapBlockProps.pins);
  const [visiblePins, setVisiblePins] = useState<MapPin[]>(
    mapBlockProps.pins,
  );
  if (prevServerPins !== mapBlockProps.pins) {
    setPrevServerPins(mapBlockProps.pins);
    setVisiblePins(mapBlockProps.pins);
  }

  // Set of artist slugs currently visible on the map. Used by
  // `mappedCount` and by the pan-aware sort below.
  const visibleSlugs = useMemo(
    () => new Set(visiblePins.map((p) => p.artist.slug)),
    [visiblePins],
  );

  // "N of M mapped" — counts items whose artist has at least one
  // pin in the current visible set. Two reasonable interpretations:
  // (a) any pin globally, (b) any pin in the visible bbox. We pick
  // (b) implicitly because `visiblePins` is whatever the map last
  // fetched, which after a pan reflects the current viewport.
  //
  // The card *order* deliberately stays put across pans — pan only
  // shifts what's visible on the map and recomputes this count,
  // never the sidebar. An earlier L4 draft tried "pan-aware sort"
  // (visible artists float to top) but the cards jumping mid-scroll
  // was disorienting; stable order wins.
  const mappedCount = useMemo(
    () => items.filter((a) => visibleSlugs.has(a.artist_slug)).length,
    [items, visibleSlugs],
  );

  // "Back to Works →" affordance — only meaningful when there are
  // items but none of them are mapped (the user is stranded on a
  // useless map view). Lives inside the caption so we don't bring
  // the disconnect-explainer's hostile copy back.
  const backToWorksHref = useMemo(() => {
    if (items.length === 0 || mappedCount > 0) return undefined;
    return searchHref(mapBlockProps.searchParams, {
      drop: ["map", "bbox"],
    });
  }, [items.length, mappedCount, mapBlockProps.searchParams]);

  return (
    <div className="lg:grid lg:grid-cols-[minmax(0,2fr)_minmax(0,3fr)] lg:gap-6 lg:items-start">
      {/* Map column. Renders first in DOM order so mobile users
          read "map, then panel" without the bottom-sheet's fixed
          positioning rearranging things for screen readers. */}
      <div className="lg:order-2 mb-12 lg:mb-0">
        <SearchMapBlock
          {...mapBlockProps}
          highlightedArtistSlug={highlightedArtistSlug}
          focusSignal={focusSignal}
          onPinsChanged={setVisiblePins}
        />
      </div>

      {/* Results panel:
          - Mobile (<lg): fixed-bottom sheet over the map, tap the
            handle to expand/collapse. Map-first mental model — the
            user sees the geography by default and pulls up the
            cards on demand.
          - Desktop (lg+): sticky left column inside the grid, the
            classic Airbnb split. */}
      <aside
        aria-label="Search results"
        className={[
          // Mobile bottom-sheet base. `dvh` so it doesn't fight the
          // mobile chrome viewport. `max-h` transition gives us the
          // peek↔expanded animation without measuring children.
          "fixed inset-x-0 bottom-0 z-30 bg-background border-t border-border shadow-2xl",
          "transition-[max-height] duration-300 ease-out",
          // `overflow-hidden` clips the inner scroll area to the
          // outer max-height — without it, peek-state shows the
          // 3rem handle but the (much taller) inner body bleeds
          // down past the viewport.
          "overflow-hidden",
          sheetExpanded ? "max-h-[70dvh]" : "max-h-12",
          // Desktop: undo all the fixed-positioning + chrome.
          "lg:static lg:order-1 lg:z-auto lg:bg-transparent lg:border-0 lg:shadow-none",
          "lg:sticky lg:top-4 lg:max-h-[640px] lg:overflow-y-auto",
          // px-1 gives the inset-shadow card highlight room to render
          // without being clipped by overflow-y-auto.
          "lg:px-1 lg:py-1",
        ].join(" ")}
      >
        {/* Mobile-only sheet handle. Mirrors the in-panel caption
            wording so the count stays visible while collapsed. */}
        <button
          type="button"
          onClick={() => setSheetExpanded((v) => !v)}
          aria-expanded={sheetExpanded}
          aria-controls="search-side-panel-body"
          className="lg:hidden flex h-12 w-full items-center justify-between border-b border-border px-4 text-xs uppercase tracking-wider text-muted"
        >
          <span>
            {items.length === 0
              ? "No results"
              : mappedCountLabel(
                  mappedCount,
                  items.length,
                  items.length >= pageLimit,
                )}
          </span>
          <span aria-hidden="true">{sheetExpanded ? "▼" : "▲"}</span>
        </button>

        {/* Panel body. On mobile, scrolls inside the fixed sheet
            (max-height accounts for the 3rem handle above). On
            desktop, the aside itself owns the scroll. */}
        <div
          id="search-side-panel-body"
          className="overflow-y-auto max-h-[calc(70dvh-3rem)] px-4 py-3 lg:max-h-none lg:overflow-visible lg:px-0 lg:py-0"
        >
          {items.length > 0 ? (
            <SearchSidePanel
              items={items}
              highlightedArtistSlug={highlightedArtistSlug}
              onHighlightArtist={setHighlightedArtistSlug}
              onFocusArtist={onFocusArtist}
              pageLimit={pageLimit}
              mappedCount={mappedCount}
              backToWorksHref={backToWorksHref}
            />
          ) : (
            emptyState
          )}
        </div>
      </aside>
    </div>
  );
}

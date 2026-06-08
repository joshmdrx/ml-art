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

import { useCallback, useMemo, useState } from "react";

import type { ArtworkSummary, MapPin, SearchParams } from "@/lib/api";
import { searchClient } from "@/lib/searchClient";
import { searchMapClient } from "@/lib/searchMapClient";
import { reportError } from "@/lib/reportError";
import { searchHref } from "@/lib/searchMap/url";

import {
  SearchMapBlock,
  type SearchMapBlockProps,
} from "./SearchMapBlock";
import type { FocusSignal } from "./SearchMap/useFocusArtist";
import { SearchSidePanel, mappedCountLabel } from "./SearchSidePanel";

interface Props {
  items: ArtworkSummary[];
  /** Cursor for the next page (from the server's `next_cursor`).
   * `null` means we've reached the end. T-037. */
  initialNextCursor: string | null;
  /** Exact params the server used for the first-page grid query.
   * The client's "Load more" hits `/v1/search` with these + the
   * current cursor so subsequent pages stay consistent. T-037. */
  gridSearchParams: SearchParams;
  /** Rendered into the side panel when `items.length === 0` (and
   * there's no error). Passed in by the page so the EmptyState
   * stays a single source of copy across grid + split views. */
  emptyState: React.ReactNode;
  mapBlockProps: Omit<
    SearchMapBlockProps,
    "highlightedArtistSlug" | "focusSignal" | "onPinsChanged"
  >;
}

export function SearchSplitView({
  items: serverItems,
  initialNextCursor,
  gridSearchParams,
  emptyState,
  mapBlockProps,
}: Props) {
  // Pagination state (T-037). Held client-side so "Load more" can
  // append without a server roundtrip. Resyncs from the server prop
  // via the prev-prop derived-state pattern whenever the page
  // re-renders with a new filter set (chip click, FilterBar change,
  // browser back/forward).
  const [prevServerItems, setPrevServerItems] = useState(serverItems);
  const [items, setItems] = useState<ArtworkSummary[]>(serverItems);
  const [nextCursor, setNextCursor] = useState<string | null>(
    initialNextCursor,
  );
  if (prevServerItems !== serverItems) {
    setPrevServerItems(serverItems);
    setItems(serverItems);
    setNextCursor(initialNextCursor);
  }

  const [loadingMore, setLoadingMore] = useState(false);
  const [loadMoreError, setLoadMoreError] = useState<string | null>(null);

  const [highlightedArtistSlug, setHighlightedArtistSlug] = useState<
    string | null
  >(null);

  // Mobile bottom-sheet expansion (L4c). Defaults to collapsed so the
  // map is fully visible on first paint — the whole point of map
  // mode on mobile is *the map*. Desktop ignores this state.
  const [sheetExpanded, setSheetExpanded] = useState(false);

  const [focusSignal, setFocusSignal] = useState<FocusSignal | null>(null);
  const onFocusArtist = (artwork: ArtworkSummary) => {
    setFocusSignal((prev) => ({
      artistSlug: artwork.artist_slug,
      artistName: artwork.artist_name,
      imageUrl: artwork.primary_image_url,
      tick: (prev?.tick ?? 0) + 1,
    }));
  };

  // Single source of truth for the map's pin set, lifted up from
  // `<SearchMap>` so we can:
  //   - compute "N of M mapped" + use it on Load More to decide
  //     whether to refresh pins for newly-introduced artists, and
  //   - inject an expanded pin set after Load More (flowing back
  //     down to SearchMap as `initial`, where useRefetchPins's
  //     prev-initial sync resets the internal pins state).
  //
  // Three update paths converge here:
  //   1. Filter change → server re-renders, mapBlockProps.pins
  //      changes → synced via the prev-prop pattern below.
  //   2. Pan refetch inside SearchMap (no-filter mode) → fires
  //      onPinsChanged → setPins.
  //   3. Load-more refetch in this component when a new page
  //      introduces an artist whose pin isn't yet loaded.
  const [prevServerPins, setPrevServerPins] = useState(mapBlockProps.pins);
  const [pins, setPins] = useState<MapPin[]>(mapBlockProps.pins);
  if (prevServerPins !== mapBlockProps.pins) {
    setPrevServerPins(mapBlockProps.pins);
    setPins(mapBlockProps.pins);
  }

  // Set of artist slugs covered by the current pin set. Used by
  // `mappedCount` and by `loadMore` (to decide if it needs to ask
  // the server for more pins).
  const visibleSlugs = useMemo(
    () => new Set(pins.map((p) => p.artist.slug)),
    [pins],
  );

  const loadMore = useCallback(async () => {
    if (!nextCursor || loadingMore) return;
    setLoadingMore(true);
    setLoadMoreError(null);
    try {
      const page = await searchClient({
        ...gridSearchParams,
        cursor: nextCursor,
      });
      setItems((prev) => [...prev, ...page.items]);
      setNextCursor(page.next_cursor ?? null);

      // Keep the map's pin set in sync with the grid pages. Without
      // this, a card whose artist first appears on a Load-more page
      // wouldn't have a pin loaded — clicking it would fall into
      // the "unmapped" branch even when the artist has a location.
      // We refetch only when the new page brings in an artist we
      // don't already have pins for, so back-to-back pages of the
      // same artists don't trigger a roundtrip.
      const introducesNewArtist = page.items.some(
        (a) => !visibleSlugs.has(a.artist_slug),
      );
      if (introducesNewArtist) {
        const allArtistIds = Array.from(
          new Set([...items, ...page.items].map((a) => a.artist_id)),
        );
        const expanded = await searchMapClient({
          artist_ids: allArtistIds.join(","),
        });
        setPins(expanded);
      }
    } catch (e) {
      reportError(e, { surface: "search-load-more" });
      setLoadMoreError(
        e instanceof Error ? e.message : "Couldn’t load more results.",
      );
    } finally {
      setLoadingMore(false);
    }
    // `items` and `visibleSlugs` are intentionally omitted from
    // deps — the button is disabled while loading, so there's no
    // realistic stale-closure risk, and listing them would create
    // a fresh callback on every render which churns child memos.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [nextCursor, loadingMore, gridSearchParams]);

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
          // Override mapBlockProps.pins with our lifted state so
          // Load-more refetches reach the map (SearchMap uses this
          // as its `initial` prop, which useRefetchPins resyncs to
          // via the prev-initial derived-state pattern).
          pins={pins}
          highlightedArtistSlug={highlightedArtistSlug}
          focusSignal={focusSignal}
          onPinsChanged={setPins}
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
                  nextCursor !== null,
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
              mappedCount={mappedCount}
              backToWorksHref={backToWorksHref}
              hasMore={nextCursor !== null}
              loadingMore={loadingMore}
              loadMoreError={loadMoreError}
              onLoadMore={loadMore}
            />
          ) : (
            emptyState
          )}
        </div>
      </aside>
    </div>
  );
}

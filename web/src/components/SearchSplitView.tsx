"use client";

/**
 * Two-column shell for `/search?map=1` (T-045 L1–L4).
 *
 * Hosts three pieces of cross-pane state:
 *
 *   - `highlightedArtistSlug` (L2) — side-panel card hover lifts the
 *     artist's pin(s) into `feature-state.highlighted = true`.
 *
 *   - `focusSignal` (L3) — `{ artistSlug, artistName, imageUrl, tick }`.
 *     Card click drives the map to flyTo + open the pin's popup. The
 *     `tick` increment re-fires the effect on repeat clicks. Also
 *     mirrored into the URL as `?focus=<artwork_id>` so back-nav
 *     from /artists/[slug] restores the same selection.
 *
 *   - `pins` (L4) — server-pushed initial set, refetched on Load More
 *     (when new pages introduce artists not yet in the pin set) +
 *     synced from SearchMap's onPinsChanged callback.
 *
 * State-restore architecture (URL-first, no sessionStorage):
 *
 *   - Pagination → `?pages=N` (server loops cursor-chained fetches).
 *     Load More pushes `?pages=N+1` via router; new render arrives.
 *   - Selected artwork → `?focus=<artwork_id>`. Set on card click
 *     via replaceState; on mount, re-fires `focusSignal` so the map
 *     + popup restore. Scrolling the matching card into view is
 *     handled below.
 *   - Filters / bbox / map mode — already in URL via existing params.
 *
 * Everything that matters for "resume my search" lives in the URL.
 * Bookmark a URL and you get exactly the view you saw.
 */

import { useCallback, useEffect, useMemo, useRef, useState, useTransition } from "react";
import { usePathname, useRouter, useSearchParams } from "next/navigation";

import type { ArtworkSummary, MapPin } from "@/lib/api";
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
  /** Rendered into the side panel when `items.length === 0` (and
   * there's no error). Passed in by the page so the EmptyState
   * stays a single source of copy across grid + split views. */
  emptyState: React.ReactNode;
  mapBlockProps: Omit<
    SearchMapBlockProps,
    "highlightedArtistSlug" | "focusSignal" | "onPinsChanged"
  >;
}

/** Mirror of `MAX_PAGES` in page.tsx — surfacing here would mean an
 * extra prop just to disable a button; the cap is small enough that
 * a server-side enforcement + a client-side soft guard cover it. */
const MAX_PAGES = 10;

export function SearchSplitView({
  items,
  initialNextCursor,
  emptyState,
  mapBlockProps,
}: Props) {
  const router = useRouter();
  const pathname = usePathname();
  const urlSearchParams = useSearchParams();

  // Pagination is URL-driven. The server already concatenated pages
  // 1..N from `?pages=N` into the `items` prop. Load More just bumps
  // the URL; the resulting server render arrives with N+1 pages.
  const currentPages = Math.max(
    1,
    Math.min(MAX_PAGES, parseInt(urlSearchParams.get("pages") ?? "1", 10) || 1),
  );
  const hasMore =
    initialNextCursor !== null && currentPages < MAX_PAGES;

  const [isPending, startTransition] = useTransition();

  const loadMore = useCallback(() => {
    if (!hasMore || isPending) return;
    const usp = new URLSearchParams(urlSearchParams.toString());
    usp.set("pages", String(currentPages + 1));
    startTransition(() => {
      // `scroll: false` so the new page renders below the current
      // scroll position without yanking the user back to the top —
      // that's the whole point of Load More vs page-N navigation.
      router.push(`${pathname}?${usp.toString()}`, { scroll: false });
    });
  }, [hasMore, isPending, urlSearchParams, currentPages, router, pathname]);

  // Highlighted artist (hover on a card → scale matching pins).
  const [highlightedArtistSlug, setHighlightedArtistSlug] = useState<
    string | null
  >(null);

  // Mobile bottom-sheet expansion (L4c). Defaults to collapsed so the
  // map is fully visible on first paint — the whole point of map
  // mode on mobile is *the map*. Desktop ignores this state.
  const [sheetExpanded, setSheetExpanded] = useState(false);

  // Focus signal drives the map's popup + flyTo. Updated by card
  // click; also mirrored into URL via `?focus=<artwork_id>` so a
  // round-trip to /artists/[slug] and back restores the selection.
  const [focusSignal, setFocusSignal] = useState<FocusSignal | null>(null);
  const onFocusArtist = useCallback(
    (artwork: ArtworkSummary) => {
      setFocusSignal((prev) => ({
        artistSlug: artwork.artist_slug,
        artistName: artwork.artist_name,
        imageUrl: artwork.primary_image_url,
        tick: (prev?.tick ?? 0) + 1,
      }));
      // Mirror into URL via replaceState — no re-render, no scroll
      // jump, just makes the URL reflect "this is the selected one"
      // so back-nav can restore. The `focus` param is consumed only
      // by the mount-time restore effect below; we don't watch it
      // during the session.
      if (typeof window === "undefined") return;
      const url = new URL(window.location.href);
      url.searchParams.set("focus", artwork.id);
      window.history.replaceState(window.history.state, "", url.toString());
    },
    [],
  );

  // Mount-time focus restore. Reads `?focus=<artwork_id>` once per
  // route key — if the artwork is in the loaded `items`, fire the
  // focus signal (so the map flies + popup opens) and scroll the
  // matching card into view in the sidebar.
  const routeKey = `${pathname}?${urlSearchParams.toString()}`;
  const restoredFocusRef = useRef<string | null>(null);
  useEffect(() => {
    if (restoredFocusRef.current === routeKey) return;
    restoredFocusRef.current = routeKey;
    const focusId = urlSearchParams.get("focus");
    if (!focusId) return;
    const artwork = items.find((a) => a.id === focusId);
    if (!artwork) return;
    // Same shape as onFocusArtist's payload, but we don't go
    // through it because we don't want to re-write the URL we just
    // read from.
    // eslint-disable-next-line react-hooks/set-state-in-effect
    setFocusSignal({
      artistSlug: artwork.artist_slug,
      artistName: artwork.artist_name,
      imageUrl: artwork.primary_image_url,
      tick: 1,
    });
    // Scroll the matching card into view. Defer to next frame so
    // the card is in the DOM and the panel's overflow-y-auto knows
    // its scrollHeight.
    requestAnimationFrame(() => {
      const card = document.querySelector(
        `[data-artwork-id="${focusId}"]`,
      );
      if (card) {
        card.scrollIntoView({ block: "nearest", behavior: "instant" });
      }
    });
  }, [routeKey, items, urlSearchParams]);

  // Single source of truth for the map's pin set, lifted up from
  // `<SearchMap>` so we can:
  //   - compute "N of M mapped" + use it on Load More to decide
  //     whether to refresh pins for newly-introduced artists, and
  //   - inject an expanded pin set after Load More (flowing back
  //     down to SearchMap as `initial`, where useRefetchPins's
  //     prev-initial sync resets the internal pins state).
  //
  // Three update paths converge here:
  //   1. Filter / pages change → server re-renders, mapBlockProps.pins
  //      changes → synced via the prev-prop pattern below.
  //   2. Pan refetch inside SearchMap (no-filter mode) → fires
  //      onPinsChanged → setPins.
  //   3. Load-more pin refetch when a new page introduces an artist
  //      we don't already have a pin for (effect below).
  const [prevServerPins, setPrevServerPins] = useState(mapBlockProps.pins);
  const [pins, setPins] = useState<MapPin[]>(mapBlockProps.pins);
  if (prevServerPins !== mapBlockProps.pins) {
    setPrevServerPins(mapBlockProps.pins);
    setPins(mapBlockProps.pins);
  }

  // Set of artist slugs covered by the current pin set. Used by
  // `mappedCount` and by the pin-refetch effect below.
  const visibleSlugs = useMemo(
    () => new Set(pins.map((p) => p.artist.slug)),
    [pins],
  );

  // When the server-pushed `items` grow (Load More returned a new
  // page with previously-unseen artists), refresh the map's pin set
  // so card-clicks on the new artists actually fly to their pins
  // rather than falling into the unmapped branch.
  useEffect(() => {
    const uncovered = items.filter((a) => !visibleSlugs.has(a.artist_slug));
    if (uncovered.length === 0) return;
    const allArtistIds = Array.from(new Set(items.map((a) => a.artist_id)));
    let cancelled = false;
    (async () => {
      try {
        const expanded = await searchMapClient({
          artist_ids: allArtistIds.join(","),
        });
        if (cancelled) return;
        setPins(expanded);
      } catch (e) {
        reportError(e, { surface: "search-loadmore-pins" });
      }
    })();
    return () => {
      cancelled = true;
    };
    // Intentionally depend only on `items` length — we want to fire
    // when pages grow, not on every render that happens to recompute
    // visibleSlugs (which would loop).
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [items.length]);

  // "N of M mapped" — counts items whose artist has at least one
  // pin in the current visible set. Card order deliberately stays
  // put across pans — pan only shifts what's visible on the map
  // and recomputes this count.
  const mappedCount = useMemo(
    () => items.filter((a) => visibleSlugs.has(a.artist_slug)).length,
    [items, visibleSlugs],
  );

  // "Back to Works →" affordance — only meaningful when there are
  // items but none of them are mapped (useless map view). Lives
  // inside the caption so we don't bring the disconnect-explainer's
  // hostile copy back.
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
          pins={pins}
          highlightedArtistSlug={highlightedArtistSlug}
          focusSignal={focusSignal}
          onPinsChanged={setPins}
        />
      </div>

      <aside
        aria-label="Search results"
        className={[
          "fixed inset-x-0 bottom-0 z-30 bg-background border-t border-border shadow-2xl",
          "transition-[max-height] duration-300 ease-out",
          "overflow-hidden",
          sheetExpanded ? "max-h-[70dvh]" : "max-h-12",
          "lg:static lg:order-1 lg:z-auto lg:bg-transparent lg:border-0 lg:shadow-none",
          "lg:sticky lg:top-4 lg:max-h-[640px] lg:overflow-y-auto",
          "lg:px-1 lg:py-1",
        ].join(" ")}
      >
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
              : mappedCountLabel(mappedCount, items.length, hasMore)}
          </span>
          <span aria-hidden="true">{sheetExpanded ? "▼" : "▲"}</span>
        </button>

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
              hasMore={hasMore}
              loadingMore={isPending}
              loadMoreError={null}
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

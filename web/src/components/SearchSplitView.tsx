"use client";

/**
 * Two-column shell for `/search?map=1` (T-045 L1 + L2).
 *
 * Holds the single piece of cross-pane state — `highlightedArtistSlug` —
 * and threads it down to both halves:
 *
 *   - `<SearchSidePanel>` reads it (to draw a ring on matching cards)
 *     and writes it (on card mouseenter / leave)
 *   - `<SearchMapBlock>` reads it and threads it to `<SearchMap>`,
 *     which applies Mapbox `feature-state.highlighted` to the
 *     matching pins
 *
 * Source-of-truth pattern: both panes derive their visual state
 * from the same `highlightedArtistSlug` value. Neither pane mutates
 * the state from a hover that originated in itself, so no ping-pong
 * loops. (Today only the panel originates hovers — pin-side
 * hover-syncs-card lands in a later slice.)
 */

import { useState } from "react";

import type { ArtworkSummary } from "@/lib/api";

import {
  SearchMapBlock,
  type SearchMapBlockProps,
} from "./SearchMapBlock";
import type { FocusSignal } from "./SearchMap/useFocusArtist";
import { SearchSidePanel } from "./SearchSidePanel";

interface Props {
  items: ArtworkSummary[];
  /** Rendered into the side panel when `items.length === 0` (and
   * there's no error). Passed in by the page so the EmptyState
   * stays a single source of copy across grid + split views. */
  emptyState: React.ReactNode;
  /** Page size used to render the "N+ works" caption when truncated. */
  pageLimit: number;
  mapBlockProps: Omit<
    SearchMapBlockProps,
    "highlightedArtistSlug" | "focusSignal"
  >;
}

export function SearchSplitView({
  items,
  emptyState,
  pageLimit,
  mapBlockProps,
}: Props) {
  // Hover-sync state (L2): scales matching pins on the map.
  const [highlightedArtistSlug, setHighlightedArtistSlug] = useState<
    string | null
  >(null);
  // Click-sync state (L3): flies the map to the artist's pin + opens
  // popup. `tick` increments on every click so re-clicks re-fire even
  // for the same artist.
  const [focusSignal, setFocusSignal] = useState<FocusSignal | null>(null);
  const onFocusArtist = (slug: string) => {
    setFocusSignal((prev) => ({
      artistSlug: slug,
      tick: (prev?.tick ?? 0) + 1,
    }));
  };

  return (
    <div className="flex flex-col gap-6 lg:grid lg:grid-cols-[minmax(0,2fr)_minmax(0,3fr)] lg:gap-6 lg:items-start">
      <aside
        aria-label="Search results"
        // px-1 gives the inset-shadow highlight room to render
        // without being clipped by overflow-y-auto. py-1 / pt-1
        // matches it vertically for the top/bottom rows.
        className="order-2 lg:order-1 lg:sticky lg:top-4 lg:max-h-[640px] lg:overflow-y-auto lg:px-1 lg:py-1"
      >
        {items.length > 0 ? (
          <SearchSidePanel
            items={items}
            highlightedArtistSlug={highlightedArtistSlug}
            onHighlightArtist={setHighlightedArtistSlug}
            onFocusArtist={onFocusArtist}
            pageLimit={pageLimit}
          />
        ) : (
          emptyState
        )}
      </aside>
      <div className="order-1 lg:order-2">
        <SearchMapBlock
          {...mapBlockProps}
          highlightedArtistSlug={highlightedArtistSlug}
          focusSignal={focusSignal}
        />
      </div>
    </div>
  );
}

"use client";

/**
 * Bridge the SearchPage's `highlightedArtistSlug` state into Mapbox's
 * per-feature `feature-state.highlighted` flag. The pin layer reads
 * that flag in its paint expressions (see `useMapPinsSource`) and
 * scales matching pins up.
 *
 * One artist can have multiple `artist_locations` (a gallery + a
 * studio in different cities, say) so a single hover may light up
 * more than one pin — that's the right thing: the user is asking
 * "where can I see this artist" and we show every place.
 *
 * T-045 L2 — card → pin direction only. The reverse (pin hover →
 * card highlight + scroll) lands separately when we need it.
 */

import { useEffect, useRef } from "react";

import type { MapPin } from "@/lib/api";

export function useHighlightedArtist(
  map: import("mapbox-gl").Map | null,
  pins: MapPin[],
  highlightedArtistSlug: string | null
): void {
  // Track which pin ids we last highlighted so we can clear them
  // cleanly. Mapbox doesn't expose "clear all states for a source"
  // — we have to remember which features we touched.
  const previouslyHighlightedRef = useRef<string[]>([]);

  useEffect(() => {
    if (!map) return;
    if (!map.getSource("pins")) return;

    // Clear last round.
    for (const id of previouslyHighlightedRef.current) {
      map.setFeatureState({ source: "pins", id }, { highlighted: false });
    }
    previouslyHighlightedRef.current = [];

    if (!highlightedArtistSlug) return;

    // Set for this round.
    const matched: string[] = [];
    for (const p of pins) {
      if (p.artist.slug === highlightedArtistSlug) {
        map.setFeatureState(
          { source: "pins", id: p.location_id },
          { highlighted: true }
        );
        matched.push(p.location_id);
      }
    }
    previouslyHighlightedRef.current = matched;
  }, [map, pins, highlightedArtistSlug]);
}

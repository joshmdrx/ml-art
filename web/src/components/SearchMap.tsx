"use client";

/**
 * `/search?map=1` map mode (T-038 G5).
 *
 * Composition root for the search map. Each concern (instance,
 * source, click handlers, bbox sync, etc.) lives in its own hook
 * under `./SearchMap/`. This file's only job is to wire them
 * together and render the chrome around the map container.
 *
 *   useMapInstance         — load Mapbox, create the Map, fitBounds
 *   useMapPinsSource       — add the clustered GeoJSON source + layers
 *   useClusterAndPinClicks — wire click → zoom-in OR list-popup OR pin-popup
 *   useMapBboxSync         — debounce moveend → URL + refetch
 *   useUrlBboxFitBounds    — react to external URL bbox changes
 *   useRefetchPins         — owns pins state, dedup, 429 handling
 *
 * No Mapbox token? `PinListFallback` renders pin cards instead so
 * the page stays useful in restricted-network environments.
 */

import { useRef } from "react";
import { useSearchParams } from "next/navigation";

import type { MapPin } from "@/lib/api";
import { NearMeButton } from "@/components/NearMeButton";

import { PinListFallback } from "./SearchMap/PinListFallback";
import { useClusterAndPinClicks } from "./SearchMap/useClusterAndPinClicks";
import { useMapBboxSync } from "./SearchMap/useMapBboxSync";
import { useMapInstance } from "./SearchMap/useMapInstance";
import { useMapPinsSource } from "./SearchMap/useMapPinsSource";
import {
  useRefetchPins,
  type MapFilters,
} from "./SearchMap/useRefetchPins";
import { useUrlBboxFitBounds } from "./SearchMap/useUrlBboxFitBounds";
import { NEAR_ME_TOP_OFFSET_PX } from "@/lib/searchMap/constants";

interface Props {
  /** Server-fetched first page of pins for the current filter set. */
  initial: MapPin[];
  /** Non-bbox filters the map should re-apply on every refetch. */
  filters: MapFilters;
}

const MAPBOX_TOKEN = process.env.NEXT_PUBLIC_MAPBOX_TOKEN;

export function SearchMap({ initial, filters }: Props) {
  if (!MAPBOX_TOKEN) {
    return <PinListFallback pins={initial} />;
  }
  return <SearchMapboxMap initial={initial} filters={filters} />;
}

function SearchMapboxMap({ initial, filters }: Props) {
  const searchParams = useSearchParams();
  const containerRef = useRef<HTMLDivElement | null>(null);

  // Pins + refetch state. Stable `refetch` reads fresh filters via
  // refs internally, so we register it once on moveend below.
  const { pins, loading, error, refetch } = useRefetchPins(initial, filters);

  // Map lifecycle. `map` is null until Mapbox's `load` event fires;
  // every downstream hook short-circuits on null.
  const { map, initError } = useMapInstance({
    containerRef,
    token: MAPBOX_TOKEN!,
    initial,
    initialBbox: searchParams.get("bbox"),
  });

  // Refetch on pan only when the user is exploring with no filter
  // set. With an active filter the server already returned every
  // matching pin (capped at 500); Mapbox handles "which pins are
  // visible" natively as the camera moves, no API call needed.
  const hasActiveFilter =
    Boolean(filters.q) ||
    Boolean(filters.medium) ||
    Boolean(filters.location) ||
    Boolean(filters.artist) ||
    Boolean(filters.artist_ids);

  useMapPinsSource(map, pins);
  useClusterAndPinClicks(map);
  useMapBboxSync(map, refetch, !hasActiveFilter);
  useUrlBboxFitBounds(map, searchParams.get("bbox") ?? "");

  const displayError = initError ?? error;

  return (
    <section className="relative">
      <div
        ref={containerRef}
        role="region"
        aria-label="Map of locations matching the current search"
        className="h-[480px] md:h-[600px] w-full border border-border bg-surface"
      />
      {/* Near-me sits in the map's chrome — top-right, below
          Mapbox's stock zoom controls. Mirrors the convention that
          Google Maps / Mapbox demos use. */}
      <div className="pointer-events-none absolute top-2 right-2 z-10 flex flex-col items-end gap-2">
        <div
          style={{ marginTop: NEAR_ME_TOP_OFFSET_PX }}
          className="pointer-events-auto"
        >
          <NearMeButton variant="map-overlay" />
        </div>
      </div>
      <div className="mt-2 flex items-center justify-between text-xs text-muted">
        <span>
          {loading
            ? "Refreshing…"
            : `${pins.length} ${pins.length === 1 ? "venue" : "venues"} in view`}
        </span>
        {displayError && (
          <span className="text-red-600">{displayError}</span>
        )}
        <span>Listed by the artists.</span>
      </div>
    </section>
  );
}

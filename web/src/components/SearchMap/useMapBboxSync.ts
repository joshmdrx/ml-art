"use client";

/**
 * Bridge the Mapbox `moveend` event to the URL bar + (optionally)
 * the pin refetch path. Owns:
 *   - debouncing moveend (so a continuous pan = one fetch)
 *   - clamping bbox to legal lat/lng ranges before sending
 *   - mirroring the active bbox into the URL via
 *     `history.replaceState` (no Next router re-render, just a URL
 *     update for bookmarkability)
 *   - calling the `refetch` callback when `refetchOnPan` is true
 *
 * `refetchOnPan` gate: when any search filter is active (`q`,
 * `medium`, `location`, `artist`, or `artist_ids`) the server has
 * already returned the *entire* matching pin set in one shot — the
 * client has all the data it needs, and Mapbox handles
 * show/hide-by-viewport natively as the camera moves. Refetching
 * on each pan in that case is wasted API work and was the original
 * source of the "many requests as you drag" complaint.
 *
 * The hook still mirrors the bbox to the URL on every pan so the
 * view stays shareable; only the network round-trip is gated.
 */

import { useEffect, useRef } from "react";

import { bboxToString, clampBbox } from "@/lib/searchMap/bbox";
import { MOVEEND_DEBOUNCE_MS } from "@/lib/searchMap/constants";

export function useMapBboxSync(
  map: import("mapbox-gl").Map | null,
  refetch: (bbox: string) => Promise<void>,
  refetchOnPan: boolean
): void {
  const refetchRef = useRef(refetch);
  const refetchOnPanRef = useRef(refetchOnPan);
  useEffect(() => {
    refetchRef.current = refetch;
  }, [refetch]);
  useEffect(() => {
    refetchOnPanRef.current = refetchOnPan;
  }, [refetchOnPan]);

  useEffect(() => {
    if (!map) return;
    let moveTimer: ReturnType<typeof setTimeout> | null = null;

    const onMoveEnd = () => {
      if (moveTimer) clearTimeout(moveTimer);
      moveTimer = setTimeout(() => {
        const b = map.getBounds();
        if (!b) return;
        const clamped = clampBbox({
          west: b.getWest(),
          south: b.getSouth(),
          east: b.getEast(),
          north: b.getNorth(),
        });
        if (!clamped) return;
        const bbox = bboxToString(clamped);

        // Mirror to URL without a full Next-router re-render.
        const next = new URLSearchParams(window.location.search);
        next.set("bbox", bbox);
        window.history.replaceState(
          null,
          "",
          `${window.location.pathname}?${next.toString()}`
        );

        if (refetchOnPanRef.current) {
          void refetchRef.current(bbox);
        }
      }, MOVEEND_DEBOUNCE_MS);
    };

    map.on("moveend", onMoveEnd);
    return () => {
      if (moveTimer) clearTimeout(moveTimer);
      map.off("moveend", onMoveEnd);
    };
  }, [map]);
}

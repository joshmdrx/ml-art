"use client";

/**
 * Re-fit the map's camera whenever the URL's `bbox` param changes
 * to a value the map *isn't already looking at*. Covers external
 * navigation (CityPivotStrip click, Near me, browser back/forward)
 * without participating in the pan-driven write loop.
 *
 * Why the approx-equal check matters:
 *   1. Our own pan handler writes bbox to the URL via
 *      `history.replaceState`. In Next 15, `useSearchParams`
 *      reacts to that — so this effect sees a "new" `urlBbox`.
 *   2. Without a guard we'd call `fitBounds`, which animates over
 *      ~600ms and then emits `moveend` at slightly-different
 *      bounds (projection rounding, padding). The handler writes
 *      THAT bbox back to the URL. This effect fires again. The
 *      map spins forever and every iteration triggers a refetch.
 *
 * Comparing the URL target to the map's *actual* current bounds
 * (with a tolerance > our write-precision) means our own writes
 * resolve as no-ops — only a true external change with a
 * meaningfully-different bbox triggers an animation.
 */

import { useEffect } from "react";

import {
  bboxesApproxEqual,
  parseBboxString,
  type Bbox,
} from "@/lib/searchMap/bbox";
import { FIT_BOUNDS_URL_PADDING } from "@/lib/searchMap/constants";

export function useUrlBboxFitBounds(
  map: import("mapbox-gl").Map | null,
  urlBbox: string
): void {
  useEffect(() => {
    if (!map || !urlBbox) return;
    const target = parseBboxString(urlBbox);
    if (!target) return;

    // Skip when the camera is already framed on (approximately) this
    // bbox. Breaks the pan → URL → fitBounds → moveend → … loop.
    const currentBounds = map.getBounds();
    if (currentBounds) {
      const current: Bbox = {
        west: currentBounds.getWest(),
        south: currentBounds.getSouth(),
        east: currentBounds.getEast(),
        north: currentBounds.getNorth(),
      };
      if (bboxesApproxEqual(current, target)) return;
    }

    map.fitBounds(
      [
        [target.west, target.south],
        [target.east, target.north],
      ],
      { padding: FIT_BOUNDS_URL_PADDING, duration: 600, maxZoom: 12 }
    );
  }, [map, urlBbox]);
}

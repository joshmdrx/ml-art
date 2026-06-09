"use client";

/**
 * Refit the camera when the server-pushed `initial` pin set changes
 * (filter applied/cleared, navigated from a city pivot, browser
 * back/forward). The dep is `initial` *identity*, not `pins` — pan
 * refetches mutate `pins` without changing `initial`, and we don't
 * want a pan inside an active filter to yank the camera around.
 *
 * Why it exists:
 *   - `useMapInstance` does an initial fit at *mount* and then
 *     deliberately ignores prop changes (the map persists across
 *     soft routes).
 *   - `useUrlBboxFitBounds` only fires when `urlBbox` is set, and
 *     only when `urlBbox` actually changed.
 *
 * That left two gaps:
 *   1. "Clear filter" pill (drops both `location` and `bbox`): pins
 *      change, urlBbox goes empty → useUrlBboxFitBounds bails. Camera
 *      stays on the previous city.
 *   2. FilterBar's facet "Location: X ×" clear (drops just location,
 *      not bbox): pins change, urlBbox is unchanged → neither fits.
 *
 * Responsibility split:
 *   - `urlBbox` *just changed* to a non-empty value → defer to
 *     `useUrlBboxFitBounds` (it's animating to that exact bbox).
 *     The chip click is this case.
 *   - Otherwise, when `initial` changes, fit to the new pin set.
 *
 * The "did urlBbox just change too?" check uses a ref to compare
 * against the previous value, since the effect only depends on
 * `initial` and `map`. We read `urlBbox` fresh each run via closure
 * and compare against the ref.
 */

import { useEffect, useRef } from "react";

import type { MapPin } from "@/lib/api";
import {
  FIT_BOUNDS_PINS_PADDING,
  WORLD_VIEW_CENTER,
  WORLD_VIEW_ZOOM,
} from "@/lib/searchMap/constants";

/**
 * Number of top-ranked pins we frame on a refit. The server returns
 * pins in display_order / relevance order, so the first few are the
 * "best" results. Using all 500 global pins zoomed the camera to
 * "world view" on a clear-filter — accurate but unfriendly.
 *
 * 5 felt right in testing: gives a tight regional view around the
 * top hit without being so narrow you can only see one pin. Tune
 * via dev-experience, not user feedback (most users never see the
 * cold-start view anyway — they arrive from a filter chain).
 */
const TOP_PINS_FOR_FIT = 5;

export function useFitToInitialPins(
  map: import("mapbox-gl").Map | null,
  initial: MapPin[],
  urlBbox: string,
): void {
  // The mount-time fit is handled in useMapInstance — skip our first
  // pass so we don't double-fit (and so we don't fight the URL bbox
  // when the user lands on a deep-linked viewport).
  const firstRunRef = useRef(true);
  const prevUrlBboxRef = useRef(urlBbox);

  useEffect(() => {
    if (!map) return;

    // Capture the urlBbox transition for this fire and then update
    // the ref so the next fire sees the right "previous" value.
    const prevUrlBbox = prevUrlBboxRef.current;
    prevUrlBboxRef.current = urlBbox;

    if (firstRunRef.current) {
      firstRunRef.current = false;
      return;
    }

    // If `urlBbox` *just transitioned to a new non-empty value*,
    // `useUrlBboxFitBounds` is already animating the camera there.
    // Don't fight it. Empty-bbox transitions still fall through —
    // useUrlBboxFitBounds bails on empty, so we own the camera.
    if (urlBbox && urlBbox !== prevUrlBbox) return;

    if (initial.length === 0) {
      // No pins to fit — pull back to the global cold-start view so
      // the user isn't stranded on the last city's blank map.
      map.flyTo({
        center: WORLD_VIEW_CENTER,
        zoom: WORLD_VIEW_ZOOM,
        duration: 600,
        essential: true,
      });
      return;
    }

    // Viewport-preserve: if the camera is already framed on at least
    // one of the new pins, leave it alone. Avoids the "I was looking
    // at London, I cleared the filter, the camera yanked to a world
    // view" papercut. Only refit when the user's current view shows
    // nothing useful from the new pin set.
    const currentBounds = map.getBounds();
    if (currentBounds) {
      const someInView = initial.some((p) =>
        currentBounds.contains([p.lng, p.lat]),
      );
      if (someInView) return;
    }

    let cancelled = false;
    (async () => {
      const mapboxgl = (await import("mapbox-gl")).default;
      if (cancelled) return;
      // Fit to the top-K most-relevant pins instead of all of them.
      // With ~500 global pins spread across continents, "fit to all"
      // is effectively "world view" — accurate but unfriendly when
      // the user wants a sense of "where do my top hits actually
      // live?". The trailing 495 pins are still rendered; they just
      // may sit off-screen until the user pans.
      const top = initial.slice(0, TOP_PINS_FOR_FIT);
      const bounds = new mapboxgl.LngLatBounds();
      for (const p of top) bounds.extend([p.lng, p.lat]);
      map.fitBounds(bounds, {
        padding: FIT_BOUNDS_PINS_PADDING,
        maxZoom: 12,
        duration: 600,
        // `essential: true` so the animation runs even when the user
        // has prefers-reduced-motion on — clearing a filter and not
        // seeing the camera respond is more disorienting than the
        // animation itself.
        essential: true,
      });
    })();
    return () => {
      cancelled = true;
    };
    // Listen on `initial` identity, NOT on urlBbox. Pan refetches
    // change urlBbox in the URL via replaceState; we must not refit
    // mid-pan. `urlBbox` is still read inside via closure to decide
    // whether to defer to useUrlBboxFitBounds — that's why it's not
    // in the dep array. The ref-compare handles the transition.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [map, initial]);
}

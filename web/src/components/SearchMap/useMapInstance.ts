"use client";

/**
 * Create + tear down a Mapbox GL JS Map instance bound to a
 * container ref. Resolves `map` only after the Map's `load` event
 * fires, so downstream hooks can blindly use `useEffect`-on-`map`
 * to attach sources / layers / listeners without worrying about
 * Mapbox's "you tried to add a source before load" warnings.
 *
 * Owns:
 *   - dynamic import of `mapbox-gl` (keeps it out of the SSR + initial
 *     client bundle; only loads on the search page in map mode)
 *   - access-token setup
 *   - NavigationControl (the +/− stack — top-right)
 *   - initial camera (world view by default; fit-to-pins if any
 *     `initial` was supplied; fit-to-bbox if the URL had one)
 *   - cleanup on unmount
 */

import { useEffect, useRef, useState } from "react";

import type { MapPin } from "@/lib/api";
import { parseBboxString } from "@/lib/searchMap/bbox";
import {
  FIT_BOUNDS_PINS_PADDING,
  FIT_BOUNDS_URL_PADDING,
  WORLD_VIEW_CENTER,
  WORLD_VIEW_ZOOM,
} from "@/lib/searchMap/constants";
import { reportError } from "@/lib/reportError";

interface UseMapInstanceInput {
  containerRef: React.RefObject<HTMLDivElement | null>;
  token: string;
  /** First-paint pins, used to choose the initial camera bounds. */
  initial: MapPin[];
  /** Initial bbox from the URL, if any. Wins over `initial`. */
  initialBbox: string | null;
}

interface UseMapInstanceOutput {
  /** `null` until the underlying Mapbox `load` event fires. */
  map: import("mapbox-gl").Map | null;
  initError: string | null;
}

export function useMapInstance({
  containerRef,
  token,
  initial,
  initialBbox,
}: UseMapInstanceInput): UseMapInstanceOutput {
  const [map, setMap] = useState<import("mapbox-gl").Map | null>(null);
  const [initError, setInitError] = useState<string | null>(null);
  const mapRef = useRef<import("mapbox-gl").Map | null>(null);

  // Effect runs once. We intentionally ignore `initial`/`initialBbox`
  // changes here — they only inform the *initial* camera. Subsequent
  // navigation is handled by other hooks.
  useEffect(() => {
    let cancelled = false;

    (async () => {
      try {
        const mapboxgl = (await import("mapbox-gl")).default;
        await import("mapbox-gl/dist/mapbox-gl.css");
        if (cancelled || !containerRef.current) return;

        mapboxgl.accessToken = token;

        const initialCenter: [number, number] = initial.length
          ? [initial[0].lng, initial[0].lat]
          : WORLD_VIEW_CENTER;
        const initialZoom = initial.length ? 9 : WORLD_VIEW_ZOOM;

        const instance = new mapboxgl.Map({
          container: containerRef.current,
          style: "mapbox://styles/mapbox/light-v11",
          center: initialCenter,
          zoom: initialZoom,
          pitchWithRotate: false,
          dragRotate: false,
        });
        mapRef.current = instance;
        instance.addControl(
          new mapboxgl.NavigationControl({ showCompass: false })
        );

        instance.on("load", () => {
          if (cancelled) return;
          // Initial framing: URL bbox > fit-to-pins > world view.
          const bbox = initialBbox ? parseBboxString(initialBbox) : null;
          if (bbox) {
            instance.fitBounds(
              [
                [bbox.west, bbox.south],
                [bbox.east, bbox.north],
              ],
              { padding: FIT_BOUNDS_URL_PADDING, duration: 0 }
            );
          } else if (initial.length >= 1) {
            // Fit to the top-5 most-relevant pins, not all of them.
            // With ~500 global pins the "fit-all" rectangle is the
            // world view — accurate but unfriendly when the user
            // wants to see where the best results actually live.
            // Mirrors `useFitToInitialPins`'s refit behaviour for
            // consistency between mount + post-mount fits.
            const top = initial.slice(0, 5);
            const bounds = new mapboxgl.LngLatBounds();
            for (const p of top) bounds.extend([p.lng, p.lat]);
            instance.fitBounds(bounds, {
              padding: FIT_BOUNDS_PINS_PADDING,
              maxZoom: 12,
              duration: 0,
            });
          }
          setMap(instance);
        });
      } catch (e) {
        reportError(e, { surface: "search-map-init" });
        if (!cancelled) setInitError("Couldn't load the map.");
      }
    })();

    return () => {
      cancelled = true;
      mapRef.current?.remove();
      mapRef.current = null;
    };
    // Mount-only. `initial`/`initialBbox`/`token`/`containerRef` are
    // intentionally read at mount and never re-read; subsequent
    // navigation goes through other hooks (useUrlBboxFitBounds + the
    // pin source's setData).
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  return { map, initError };
}

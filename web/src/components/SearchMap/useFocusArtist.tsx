"use client";

/**
 * When the side panel signals "focus this artist on the map" (card
 * click), open the artist's popup and (when possible) fly the camera
 * to their pin. T-045 L3 + the unmapped-artist follow-up.
 *
 * Two cases:
 *
 *   - **Mapped** — the artist has at least one pin in the current
 *     pin set. Open a `<PinPopup>` at the pin's coordinates and
 *     start a `flyTo`. Mapbox keeps the popup pinned to its
 *     `setLngLat` location as the camera animates, so the popup
 *     tracks the pin into view even when the user is starting far
 *     away.
 *   - **Unmapped** — the artist has no pin in the current set
 *     (they haven't shared a public location). Open an
 *     `<UnmappedArtistPopup>` at the map's current center with a
 *     "Not on the map yet" subhead + a portfolio link. The popup
 *     uses the clicked artwork's image (carried on the signal)
 *     since there's no venue thumbnail to fall back on. No flyTo —
 *     there's nowhere meaningful to go.
 *
 * The signal carries a monotonic `tick` so successive clicks on the
 * same card re-fire the effect.
 *
 * Robustness note (mapped path): opening the popup *immediately* and
 * letting Mapbox track it during the flight is more robust than the
 * earlier `map.once("moveend", open)` approach, which could race the
 * URL-bbox sync's debounced handler and leave the popup unopened
 * when the pin was offscreen.
 */

import { useEffect, useRef } from "react";

import type { MapPin } from "@/lib/api";
import type { PinProperties } from "@/lib/searchMap/geojson";

import { PinPopup, UnmappedArtistPopup, mountPopupContent } from "./popups";

export interface FocusSignal {
  artistSlug: string;
  /** Display name of the clicked artist. Used by the unmapped
   * popup, which can't read from a `MapPin` (there isn't one). */
  artistName: string;
  /** The clicked *artwork's* image URL. Used by the unmapped popup
   * as a context-relevant thumbnail — the user just clicked this
   * artwork, so it's a stronger visual anchor than the artist's
   * default thumbnail (which we don't carry here anyway). */
  imageUrl: string | null;
  /** Incremented by the caller on each click. Lets us re-trigger the
   * fly even when the user re-clicks the same artist. */
  tick: number;
}

export function useFocusArtist(
  map: import("mapbox-gl").Map | null,
  pins: MapPin[],
  signal: FocusSignal | null
): void {
  // Track the popup we opened so we can close it before opening a
  // new one — clicking through several cards in a row shouldn't
  // leave a pile of popups on the map.
  const activePopupRef = useRef<import("mapbox-gl").Popup | null>(null);

  useEffect(() => {
    if (!map || !signal) return;

    // Find the artist's pin. Multi-location artists: use the first
    // one (the map endpoint already orders by display_order). That
    // keeps the behaviour predictable; a future "show all locations
    // for this artist" affordance can sit in the popup.
    const pin = pins.find((p) => p.artist.slug === signal.artistSlug);

    let cancelled = false;

    (async () => {
      const mapboxgl = (await import("mapbox-gl")).default;
      if (cancelled) return;

      // Close the previous popup (if any) before opening a new one.
      activePopupRef.current?.remove();
      activePopupRef.current = null;

      if (pin) {
        // ── Mapped path ────────────────────────────────────────────
        // Open the popup right away. Mapbox keeps the popup pinned
        // to `setLngLat(coords)` as the camera animates, so during
        // the flyTo below the popup tracks the pin into view —
        // even when the user is starting far away from the target.
        const coords: [number, number] = [pin.lng, pin.lat];
        const props: PinProperties = {
          location_id: pin.location_id,
          name: pin.name,
          kind: pin.kind,
          address_city: pin.city ?? "",
          artist_slug: pin.artist.slug,
          artist_name: pin.artist.display_name,
          artist_image: pin.artist.primary_image_url ?? "",
        };
        const { element, unmount } = mountPopupContent(
          <PinPopup props={props} />,
        );
        const popup = new mapboxgl.Popup({ offset: 14, closeButton: true })
          .setLngLat(coords)
          .setDOMContent(element)
          .on("close", unmount)
          .addTo(map);
        activePopupRef.current = popup;

        // Fly the camera. `essential: true` overrides
        // `prefers-reduced-motion` (the user clicked, they expect a
        // visible response). `zoom: max(current, 11)` keeps the
        // user from being zoomed *out* if they were already close,
        // while still pulling them in to street level when they
        // were panned far away.
        //
        // Speed + duration cap: Mapbox's default flyTo is a
        // "scenic" zoom-out-then-in arc that can run 4–6 seconds
        // from a global view → a single city — slow enough that
        // the bbox URL write (which fires on `moveend`) feels
        // sluggish too. Cap at 1.2s and lower `curve` so the arc
        // is closer to a straight line. Still uses a fly rather
        // than an instant `jumpTo` because the path-of-the-camera
        // is the "I'm taking you to this pin" cue.
        map.flyTo({
          center: coords,
          zoom: Math.max(map.getZoom(), 11),
          speed: 2.0,
          curve: 1.1,
          maxDuration: 1200,
          essential: true,
        });
      } else {
        // ── Unmapped path ──────────────────────────────────────────
        // No pin for this artist. Surface a popup at the map's
        // current center explaining the situation + offering a
        // portfolio link. No flyTo — there's nowhere meaningful to
        // go. Anchoring at `getCenter()` keeps the popup in the
        // user's eyeline regardless of where they've panned to.
        const center = map.getCenter();
        const { element, unmount } = mountPopupContent(
          <UnmappedArtistPopup
            artistSlug={signal.artistSlug}
            artistName={signal.artistName}
            imageUrl={signal.imageUrl}
          />,
        );
        const popup = new mapboxgl.Popup({ offset: 14, closeButton: true })
          .setLngLat(center)
          .setDOMContent(element)
          .on("close", unmount)
          .addTo(map);
        activePopupRef.current = popup;
      }
    })();

    return () => {
      cancelled = true;
    };
    // We deliberately key off `signal.tick` so re-clicks re-fire,
    // and intentionally exclude `pins` so a refetch mid-click
    // doesn't cancel an in-flight fly.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [map, signal?.tick]);
}

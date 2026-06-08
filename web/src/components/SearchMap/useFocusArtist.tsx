"use client";

/**
 * When the side panel signals "focus this artist on the map" (card
 * click), open the artist's popup and fly the camera to it. T-045 L3.
 *
 * The signal is a `{ slug, tick }` pair rather than just a slug
 * string so successive clicks on the *same* card still re-fly to
 * the pin (a bare slug wouldn't change, so the effect wouldn't
 * re-run).
 *
 * Implementation note: we open the popup *immediately* using the
 * pin's geographic coordinates, then start the `flyTo`. Mapbox
 * popups stay anchored to their `setLngLat` location as the camera
 * moves, so the popup naturally follows the flight in to the pin.
 * This is more robust than trying to wait for `moveend` — the
 * earlier version racing `once("moveend")` against the URL-bbox
 * sync's debounced handler could leave the popup unopened when the
 * pin was off-screen.
 */

import { useEffect, useRef } from "react";

import type { MapPin } from "@/lib/api";
import type { PinProperties } from "@/lib/searchMap/geojson";

import { PinPopup, mountPopupContent } from "./popups";

export interface FocusSignal {
  artistSlug: string;
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
    if (!pin) return;

    let cancelled = false;

    (async () => {
      const mapboxgl = (await import("mapbox-gl")).default;
      if (cancelled) return;

      // Close the previous popup (if any) before opening a new one.
      activePopupRef.current?.remove();
      activePopupRef.current = null;

      const coords: [number, number] = [pin.lng, pin.lat];

      // Open the popup right away. Mapbox keeps the popup pinned to
      // `setLngLat(coords)` as the camera animates, so during the
      // flyTo below the popup tracks the pin into view — even when
      // the user is starting far away from the target.
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
        <PinPopup props={props} />
      );
      const popup = new mapboxgl.Popup({ offset: 14, closeButton: true })
        .setLngLat(coords)
        .setDOMContent(element)
        .on("close", unmount)
        .addTo(map);
      activePopupRef.current = popup;

      // Fly the camera. `essential: true` overrides
      // `prefers-reduced-motion` (the user clicked, they expect a
      // visible response). `zoom: max(current, 11)` keeps the user
      // from being zoomed *out* if they were already close, while
      // still pulling them in to street level when they were panned
      // far away.
      map.flyTo({
        center: coords,
        zoom: Math.max(map.getZoom(), 11),
        speed: 1.2,
        curve: 1.4,
        essential: true,
      });
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

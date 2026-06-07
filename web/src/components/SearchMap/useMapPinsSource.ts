"use client";

/**
 * Register the clustered `pins` source + its three layers (cluster
 * circles, cluster counts, individual pins) on the given Mapbox
 * instance. Keeps the source's `data` in sync with the current
 * `pins` prop via `setData` — no source rebuild, no flicker.
 *
 * Caller is responsible for passing the *post-load* Map (use
 * `useMapInstance`, which only resolves after `load`). This hook
 * doesn't guard for load state.
 */

import { useEffect } from "react";

import type { MapPin } from "@/lib/api";
import {
  CLUSTER_MAX_ZOOM,
  CLUSTER_RADIUS_PX,
} from "@/lib/searchMap/constants";
import { toFeatureCollection } from "@/lib/searchMap/geojson";

export function useMapPinsSource(
  map: import("mapbox-gl").Map | null,
  pins: MapPin[]
): void {
  // Register source + layers once when the map first becomes
  // available. Subsequent pin changes flow through the next effect.
  useEffect(() => {
    if (!map) return;
    if (map.getSource("pins")) return; // idempotent on remount

    map.addSource("pins", {
      type: "geojson",
      data: toFeatureCollection(pins),
      cluster: true,
      clusterRadius: CLUSTER_RADIUS_PX,
      clusterMaxZoom: CLUSTER_MAX_ZOOM,
    });

    map.addLayer({
      id: "clusters",
      type: "circle",
      source: "pins",
      filter: ["has", "point_count"],
      paint: {
        "circle-color": "#222",
        "circle-radius": ["step", ["get", "point_count"], 14, 10, 18, 50, 22],
        "circle-stroke-color": "#fff",
        "circle-stroke-width": 2,
      },
    });
    map.addLayer({
      id: "cluster-count",
      type: "symbol",
      source: "pins",
      filter: ["has", "point_count"],
      layout: { "text-field": ["get", "point_count"], "text-size": 12 },
      paint: { "text-color": "#fff" },
    });
    map.addLayer({
      id: "unclustered-point",
      type: "circle",
      source: "pins",
      filter: ["!", ["has", "point_count"]],
      paint: {
        "circle-color": "#222",
        "circle-radius": 7,
        "circle-stroke-color": "#fff",
        "circle-stroke-width": 2,
      },
    });

    // Cursor affordances. Both layers get the same treatment.
    for (const layer of ["clusters", "unclustered-point"]) {
      map.on("mouseenter", layer, () => {
        map.getCanvas().style.cursor = "pointer";
      });
      map.on("mouseleave", layer, () => {
        map.getCanvas().style.cursor = "";
      });
    }
    // Mount-once: we don't want to recreate the source on pin
    // changes — the next effect updates it in place.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [map]);

  // Keep the source's `data` in sync with `pins`.
  useEffect(() => {
    if (!map) return;
    const src = map.getSource("pins") as
      | import("mapbox-gl").GeoJSONSource
      | undefined;
    if (!src) return;
    src.setData(toFeatureCollection(pins));
  }, [map, pins]);
}

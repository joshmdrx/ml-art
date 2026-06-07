"use client";

/**
 * Wire up the cluster + single-pin click handlers. Owns the
 * leaf-coincidence decision tree (zoom-in vs list-popup) and the
 * `Popup.setDOMContent` React-island lifecycle for both popup
 * shapes.
 */

import { useEffect } from "react";

import { CLUSTER_LEAVES_LIMIT } from "@/lib/searchMap/constants";
import { leavesAreCoincident, pointCoords } from "@/lib/searchMap/cluster";
import type { PinProperties } from "@/lib/searchMap/geojson";

import {
  ClusterListPopup,
  PinPopup,
  mountPopupContent,
} from "./popups";

export function useClusterAndPinClicks(
  map: import("mapbox-gl").Map | null
): void {
  useEffect(() => {
    if (!map) return;

    let cancelled = false;
    let cleanup: (() => void) | null = null;

    // Dynamic import keeps mapbox-gl out of the SSR + non-map-page
    // bundles. We grab the Popup constructor here rather than at
    // module top level.
    (async () => {
      const mapboxgl = (await import("mapbox-gl")).default;
      if (cancelled) return;

      const onClusterClick = (e: import("mapbox-gl").MapMouseEvent) => {
        const features = map.queryRenderedFeatures(e.point, {
          layers: ["clusters"],
        });
        const cluster = features[0];
        if (!cluster?.properties) return;
        const clusterId = cluster.properties.cluster_id as number;
        const pointCount = Number(cluster.properties.point_count ?? 0);
        const source = map.getSource(
          "pins"
        ) as import("mapbox-gl").GeoJSONSource;

        source.getClusterLeaves(
          clusterId,
          pointCount || CLUSTER_LEAVES_LIMIT,
          0,
          (leafErr, leaves) => {
            if (leafErr || !leaves || leaves.length === 0) return;
            const typedLeaves = leaves as GeoJSON.Feature<
              GeoJSON.Geometry,
              PinProperties
            >[];

            if (leavesAreCoincident(typedLeaves)) {
              const coords = pointCoords(cluster.geometry);
              if (!coords) return;
              const { element, unmount } = mountPopupContent(
                <ClusterListPopup leaves={typedLeaves} />
              );
              new mapboxgl.Popup({ offset: 14, closeButton: true })
                .setLngLat(coords)
                .setDOMContent(element)
                .on("close", unmount)
                .addTo(map);
              return;
            }
            // Organic cluster — zoom in to break it apart.
            source.getClusterExpansionZoom(
              clusterId,
              (zErr, expansionZoom) => {
                if (zErr || expansionZoom == null) return;
                const coords = pointCoords(cluster.geometry);
                if (coords)
                  map.easeTo({ center: coords, zoom: expansionZoom });
              }
            );
          }
        );
      };

      const onPinClick = (e: import("mapbox-gl").MapMouseEvent) => {
        const features = (
          e as import("mapbox-gl").MapMouseEvent & {
            features?: GeoJSON.Feature<GeoJSON.Geometry, PinProperties>[];
          }
        ).features;
        if (!features || features.length === 0) return;
        const f = features[0];
        const coords = pointCoords(f.geometry);
        if (!coords) return;
        const { element, unmount } = mountPopupContent(
          <PinPopup props={f.properties} />
        );
        new mapboxgl.Popup({ offset: 14, closeButton: true })
          .setLngLat(coords)
          .setDOMContent(element)
          .on("close", unmount)
          .addTo(map);
      };

      map.on("click", "clusters", onClusterClick);
      map.on("click", "unclustered-point", onPinClick);

      cleanup = () => {
        map.off("click", "clusters", onClusterClick);
        map.off("click", "unclustered-point", onPinClick);
      };
    })();

    return () => {
      cancelled = true;
      cleanup?.();
    };
  }, [map]);
}

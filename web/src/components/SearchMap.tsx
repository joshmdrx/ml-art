"use client";

/**
 * T-038 G5 — `/search?map=1` map mode.
 *
 * Client component that owns the Mapbox GL JS map for the search page.
 * Receives an `initial` pin list (server-rendered with the URL's
 * filters) and re-fetches as the user pans / zooms by calling
 * `/v1/search/map?bbox=…` directly from the browser. Bounds are
 * synced back to the URL so views are shareable.
 *
 * Render paths mirror `ArtistLocationsMap`:
 *   - no token → non-interactive list of pin cards
 *   - token present → real GL JS map with clustering
 *
 * Clustering is done client-side via the standard Mapbox GeoJSON
 * source `cluster: true` config — works at any zoom level and
 * doesn't require server-side aggregation. Up to 500 pins per
 * response (server cap) is well within Mapbox's smooth-clustering
 * range.
 */

import { useCallback, useEffect, useRef, useState } from "react";
import { useSearchParams } from "next/navigation";
import type { MapPin } from "@/lib/api";
import { searchMapClient } from "@/lib/searchMapClient";
import { reportError } from "@/lib/reportError";

interface Props {
  /** Server-fetched first page of pins for the current filter set. */
  initial: MapPin[];
  /** The non-bbox filters carried in the URL — re-applied on every
   * client-side refetch so panning the map doesn't drop them. */
  filters: {
    q?: string;
    medium?: string;
    location?: string;
    /** Set by the "See on map" CTA on `/artists/[slug]`. When present,
     * the map only shows that artist's venues. */
    artist?: string;
    /** Comma-joined uuid list — the "map = view of grid result"
     * thread-through. Persisted across pan/zoom so the artists pinned
     * on first paint stay pinned as the user moves the map. */
    artist_ids?: string;
  };
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
  const mapRef = useRef<import("mapbox-gl").Map | null>(null);
  const [pins, setPins] = useState<MapPin[]>(initial);
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(false);

  // Refetch with the current bbox + filters. Debounced via the
  // moveend event throttle on Mapbox's side; we don't add another
  // layer of debounce here.
  const refetch = useCallback(
    async (bbox: string) => {
      setLoading(true);
      try {
        const next = await searchMapClient({
          q: filters.q,
          medium: filters.medium,
          location: filters.location,
          artist: filters.artist,
          artist_ids: filters.artist_ids,
          bbox,
        });
        setPins(next);
        setError(null);
      } catch (e) {
        reportError(e, { surface: "search-map-refetch" });
        setError(e instanceof Error ? e.message : "Couldn't refresh map");
      } finally {
        setLoading(false);
      }
    },
    [
      filters.q,
      filters.medium,
      filters.location,
      filters.artist,
      filters.artist_ids,
    ]
  );

  // One-time map setup.
  useEffect(() => {
    let cancelled = false;

    (async () => {
      try {
        const mapboxgl = (await import("mapbox-gl")).default;
        await import("mapbox-gl/dist/mapbox-gl.css");
        if (cancelled || !containerRef.current) return;

        mapboxgl.accessToken = MAPBOX_TOKEN!;

        // Initial view: if URL has bbox, use it; if we have pins, fit
        // to them; otherwise default to a wide world view.
        const urlBbox = searchParams.get("bbox");
        const initialCenter: [number, number] = initial.length
          ? [initial[0].lng, initial[0].lat]
          : [0, 30];
        const initialZoom = initial.length ? 9 : 1.4;

        const map = new mapboxgl.Map({
          container: containerRef.current,
          style: "mapbox://styles/mapbox/light-v11",
          center: initialCenter,
          zoom: initialZoom,
          pitchWithRotate: false,
          dragRotate: false,
        });
        mapRef.current = map;

        map.addControl(new mapboxgl.NavigationControl({ showCompass: false }));

        map.on("load", () => {
          // GeoJSON source with clustering. We update its `data` as
          // pins change rather than recreating the source.
          map.addSource("pins", {
            type: "geojson",
            data: toFeatureCollection(initial),
            cluster: true,
            clusterRadius: 50,
            clusterMaxZoom: 14,
          });

          // Cluster circles.
          map.addLayer({
            id: "clusters",
            type: "circle",
            source: "pins",
            filter: ["has", "point_count"],
            paint: {
              "circle-color": "#222",
              "circle-radius": [
                "step",
                ["get", "point_count"],
                14,
                10,
                18,
                50,
                22,
              ],
              "circle-stroke-color": "#fff",
              "circle-stroke-width": 2,
            },
          });
          map.addLayer({
            id: "cluster-count",
            type: "symbol",
            source: "pins",
            filter: ["has", "point_count"],
            layout: {
              "text-field": ["get", "point_count"],
              "text-size": 12,
            },
            paint: { "text-color": "#fff" },
          });

          // Individual pins.
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

          // Click cluster → zoom in, OR if the underlying points are
          // coincident (same lat/lng — e.g. multiple demo galleries
          // pinned to the same city centroid), show a list popup of
          // the leaves instead. Without this fallback the cluster
          // re-clusters at every zoom level and the click does
          // nothing visible past street zoom.
          map.on("click", "clusters", (e) => {
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

            source.getClusterExpansionZoom(clusterId, (err, expansionZoom) => {
              if (err || expansionZoom == null) return;
              const currentZoom = map.getZoom();
              // If clustering would still group these points even
              // after we zoom in (i.e. expansionZoom isn't actually
              // higher than where we are), pull the leaves directly
              // and render a list popup.
              const cantExpandFurther = expansionZoom <= currentZoom + 0.01;
              if (cantExpandFurther) {
                source.getClusterLeaves(
                  clusterId,
                  pointCount || 50, // cap at 50 — enough for any same-city cluster
                  0,
                  (leafErr, leaves) => {
                    if (leafErr || !leaves || leaves.length === 0) return;
                    const coords = pointCoords(cluster.geometry);
                    if (!coords) return;
                    new mapboxgl.Popup({ offset: 14, closeButton: true })
                      .setLngLat(coords)
                      .setHTML(renderClusterListHtml(leaves))
                      .addTo(map);
                  }
                );
              } else {
                const coords = pointCoords(cluster.geometry);
                if (coords) map.easeTo({ center: coords, zoom: expansionZoom });
              }
            });
          });
          map.on("mouseenter", "clusters", () => {
            map.getCanvas().style.cursor = "pointer";
          });
          map.on("mouseleave", "clusters", () => {
            map.getCanvas().style.cursor = "";
          });

          // Click individual pin → popup.
          map.on("click", "unclustered-point", (e) => {
            const features = e.features;
            if (!features || features.length === 0) return;
            const f = features[0];
            const props = (f.properties ?? {}) as Record<string, unknown>;
            const coords = pointCoords(f.geometry);
            if (!coords) return;
            new mapboxgl.Popup({ offset: 14, closeButton: true })
              .setLngLat(coords)
              .setHTML(
                renderPinPopupHtml({
                  name: String(props.name ?? ""),
                  kind: String(props.kind ?? "gallery"),
                  address_city: String(props.address_city ?? ""),
                  artist_slug: String(props.artist_slug ?? ""),
                  artist_name: String(props.artist_name ?? ""),
                  artist_image: String(props.artist_image ?? ""),
                })
              )
              .addTo(map);
          });
          map.on("mouseenter", "unclustered-point", () => {
            map.getCanvas().style.cursor = "pointer";
          });
          map.on("mouseleave", "unclustered-point", () => {
            map.getCanvas().style.cursor = "";
          });
        });

        // Wire pan/zoom → URL bbox + refetch.
        map.on("moveend", () => {
          const b = map.getBounds();
          if (!b) return;
          const bbox = [
            b.getWest(),
            b.getSouth(),
            b.getEast(),
            b.getNorth(),
          ]
            .map((n) => n.toFixed(4))
            .join(",");

          // Replace `bbox` in the URL without scrolling or pushing a
          // history entry. `router.replace` here would re-render the
          // server component (expensive); use `history.replaceState`
          // so only the URL changes.
          const next = new URLSearchParams(window.location.search);
          next.set("bbox", bbox);
          window.history.replaceState(
            null,
            "",
            `${window.location.pathname}?${next.toString()}`
          );

          void refetch(bbox);
        });

        // If the URL already had a bbox on first load, fit to it; else
        // if we have pins, fit to those.
        if (urlBbox) {
          const parts = urlBbox.split(",").map(Number);
          if (parts.length === 4 && parts.every(Number.isFinite)) {
            map.fitBounds(
              [
                [parts[0], parts[1]],
                [parts[2], parts[3]],
              ],
              { padding: 40, duration: 0 }
            );
          }
        } else if (initial.length >= 1) {
          // Fit to all pins — including the single-pin case. Without
          // `>= 1`, a 1-result search stays at the world view (z=1.4)
          // and the pin is invisible. We already nudge `initialZoom`
          // to 9 in that case, but fitBounds gives the correct
          // framing.
          const bounds = new mapboxgl.LngLatBounds();
          for (const p of initial) bounds.extend([p.lng, p.lat]);
          map.fitBounds(bounds, { padding: 60, maxZoom: 12, duration: 0 });
        }
      } catch (e) {
        reportError(e, { surface: "search-map-init" });
        if (!cancelled) setError("Couldn't load the map.");
      }
    })();

    return () => {
      cancelled = true;
      mapRef.current?.remove();
      mapRef.current = null;
    };
    // We intentionally exclude `searchParams`/`initial`/`refetch` from
    // deps: this effect builds the map once. Subsequent pin updates
    // flow through the next effect via `setData`.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  // When pins change, update the GeoJSON source in place — no
  // re-init, no flicker.
  useEffect(() => {
    const map = mapRef.current;
    if (!map) return;
    const src = map.getSource("pins") as
      | import("mapbox-gl").GeoJSONSource
      | undefined;
    if (!src) return;
    src.setData(toFeatureCollection(pins));
  }, [pins]);

  // When the server hands us a fresh `initial` (URL bbox / filters
  // changed → page re-rendered with new server data), pull it into
  // the local `pins` state. Without this, clicking a CityPivotStrip
  // link or "Near me" updates the URL + server but the map stays
  // stuck on the first-mount pins.
  useEffect(() => {
    setPins(initial);
  }, [initial]);

  // When the URL bbox changes after mount (city pivot click, Near me,
  // back/forward nav), recenter the map on that bbox. Without this,
  // the URL updates but the camera stays on the previous view.
  const urlBboxKey = searchParams.get("bbox") ?? "";
  useEffect(() => {
    const map = mapRef.current;
    if (!map || !urlBboxKey) return;
    const parts = urlBboxKey.split(",").map(Number);
    if (parts.length !== 4 || !parts.every(Number.isFinite)) return;
    map.fitBounds(
      [
        [parts[0], parts[1]],
        [parts[2], parts[3]],
      ],
      { padding: 40, duration: 600, maxZoom: 12 }
    );
  }, [urlBboxKey]);

  return (
    <section className="relative">
      <div
        ref={containerRef}
        role="region"
        aria-label="Map of locations matching the current search"
        className="h-[480px] md:h-[600px] w-full border border-border bg-surface"
      />
      <div className="mt-2 flex items-center justify-between text-xs text-muted">
        <span>
          {loading
            ? "Refreshing…"
            : `${pins.length} ${pins.length === 1 ? "venue" : "venues"} in view`}
        </span>
        {error && <span className="text-red-600">{error}</span>}
        <span>Listed by the artists.</span>
      </div>
    </section>
  );
}

// ─────────────────────────────────────────────────────────────────────────────
// GeoJSON conversion + popup HTML
// ─────────────────────────────────────────────────────────────────────────────

/** Narrow a GeoJSON Geometry to a Point's `[lng, lat]`, or null if it's
 * a non-Point (line, polygon, geometry collection). Mapbox typings
 * widen feature geometry to the full union; we only ever insert
 * Points, so this is a runtime guard rather than a real branch. */
function pointCoords(g: GeoJSON.Geometry): [number, number] | null {
  if (g.type === "Point" && Array.isArray(g.coordinates)) {
    const [lng, lat] = g.coordinates;
    if (typeof lng === "number" && typeof lat === "number") return [lng, lat];
  }
  return null;
}

function toFeatureCollection(pins: MapPin[]): GeoJSON.FeatureCollection {
  return {
    type: "FeatureCollection",
    features: pins.map((p) => ({
      type: "Feature",
      geometry: {
        type: "Point",
        coordinates: [p.lng, p.lat],
      },
      properties: {
        location_id: p.location_id,
        name: p.name,
        kind: p.kind,
        address_city: p.city ?? "",
        artist_slug: p.artist.slug,
        artist_name: p.artist.display_name,
        artist_image: p.artist.primary_image_url ?? "",
      },
    })),
  };
}

/** Popup body for a cluster of coincident pins — a compact list of
 * each underlying venue. Triggered when the cluster can't expand
 * any further (multiple artists sharing the same lat/lng, common
 * for the demo seed where every gallery is anchored on a city
 * centroid). Each row is a clickable artist link so the user can
 * dive into a portfolio without having to dismiss the popup. */
function renderClusterListHtml(
  leaves: GeoJSON.Feature<GeoJSON.Geometry>[]
): string {
  const rows = leaves
    .map((leaf) => {
      const p = (leaf.properties ?? {}) as Record<string, unknown>;
      const name = String(p.name ?? "");
      const kindRaw = String(p.kind ?? "gallery");
      const kindLabel = kindRaw === "gallery" ? "Gallery" : "Studio";
      const artistName = String(p.artist_name ?? "");
      const artistSlug = String(p.artist_slug ?? "");
      return `
        <li style="padding: 8px 0; border-top: 1px solid #eee;">
          <p style="font-size: 10px; letter-spacing: 0.08em; text-transform: uppercase; color: #666; margin: 0;">${kindLabel}</p>
          <p style="font-weight: 600; margin: 2px 0 0;">${escapeHtml(name)}</p>
          <a href="/artists/${encodeURIComponent(artistSlug)}" style="font-size: 13px; text-decoration: underline;">${escapeHtml(artistName)} →</a>
        </li>`;
    })
    .join("");
  const headerLabel = `${leaves.length} venues at this location`;
  return `
    <div style="font-family: inherit; min-width: 220px; max-width: 280px;">
      <p style="font-size: 11px; letter-spacing: 0.08em; text-transform: uppercase; color: #666; margin: 0 0 4px;">${headerLabel}</p>
      <ul style="list-style: none; padding: 0; margin: 0; max-height: 320px; overflow-y: auto;">
        ${rows}
      </ul>
    </div>
  `;
}

function renderPinPopupHtml(props: {
  name: string;
  kind: string;
  address_city: string;
  artist_slug: string;
  artist_name: string;
  artist_image: string;
}): string {
  const kindLabel = props.kind === "gallery" ? "Gallery" : "Studio";
  const thumb = props.artist_image
    ? `<img src="${escapeAttr(props.artist_image)}" alt="" style="width: 100%; height: 96px; object-fit: cover; margin-bottom: 8px;" />`
    : "";
  return `
    <div style="font-family: inherit; min-width: 200px; max-width: 240px;">
      ${thumb}
      <p style="font-size: 10px; letter-spacing: 0.08em; text-transform: uppercase; color: #666; margin: 0;">${kindLabel}${props.address_city ? ` · ${escapeHtml(props.address_city)}` : ""}</p>
      <p style="font-weight: 600; margin: 4px 0 0;">${escapeHtml(props.name)}</p>
      <p style="font-size: 13px; color: #444; margin: 2px 0;">Showing ${escapeHtml(props.artist_name)}</p>
      <a href="/artists/${encodeURIComponent(props.artist_slug)}" style="display: inline-block; margin-top: 6px; text-decoration: underline; font-size: 13px;">View portfolio →</a>
    </div>
  `;
}

function escapeHtml(s: string): string {
  return s
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;")
    .replace(/"/g, "&quot;")
    .replace(/'/g, "&#39;");
}
function escapeAttr(s: string): string {
  return escapeHtml(s);
}

// ─────────────────────────────────────────────────────────────────────────────
// Fallback list view (no token)
// ─────────────────────────────────────────────────────────────────────────────

function PinListFallback({ pins }: { pins: MapPin[] }) {
  if (pins.length === 0) {
    return (
      <div className="py-24 text-center text-muted">
        No venues match this search.
      </div>
    );
  }
  return (
    <ul className="grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-3 gap-4">
      {pins.map((p) => (
        <li key={p.location_id} className="border border-border bg-surface p-4">
          <p className="text-[10px] uppercase tracking-wider text-muted">
            {p.kind}
            {p.city ? ` · ${p.city}` : ""}
          </p>
          <p className="mt-1 font-medium">{p.name}</p>
          <p className="text-sm text-muted">
            Showing{" "}
            <a
              href={`/artists/${encodeURIComponent(p.artist.slug)}`}
              className="underline"
            >
              {p.artist.display_name}
            </a>
          </p>
        </li>
      ))}
    </ul>
  );
}

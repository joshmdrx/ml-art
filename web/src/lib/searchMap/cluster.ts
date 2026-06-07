/**
 * Pure helpers for the search-map cluster click flow.
 *
 * The interesting decision is: when a cluster is clicked, do we
 * zoom in (organic geographic spread, fine to break apart) or pop
 * a list of leaves (coincident pins that won't separate at any
 * zoom level)? Detection lives here so it can be unit-tested
 * without instantiating Mapbox.
 */

import { COINCIDENT_EPSILON_DEG } from "./constants";

/**
 * Are all GeoJSON Point leaves within {@link COINCIDENT_EPSILON_DEG}
 * of each other on both lat and lng? Used to switch from
 * "zoom-in-to-break-apart" to "list-popup" behaviour on cluster
 * click. Single-feature and non-Point inputs return `false`
 * (callers shouldn't reach for the list popup in those cases).
 */
export function leavesAreCoincident(
  leaves: GeoJSON.Feature<GeoJSON.Geometry>[]
): boolean {
  if (leaves.length < 2) return false;
  const first = leaves[0].geometry;
  if (first.type !== "Point") return false;
  const [lng0, lat0] = first.coordinates as [number, number];
  return leaves.every((leaf) => {
    if (leaf.geometry.type !== "Point") return false;
    const [lng, lat] = leaf.geometry.coordinates as [number, number];
    return (
      Math.abs(lng - lng0) < COINCIDENT_EPSILON_DEG &&
      Math.abs(lat - lat0) < COINCIDENT_EPSILON_DEG
    );
  });
}

/**
 * Narrow a GeoJSON Geometry to a Point's `[lng, lat]`, or null if
 * it's a non-Point (line, polygon, geometry collection). Mapbox
 * typings widen feature geometry to the full union; we only ever
 * insert Points, so this is a runtime guard rather than a real
 * branch.
 */
export function pointCoords(g: GeoJSON.Geometry): [number, number] | null {
  if (g.type === "Point" && Array.isArray(g.coordinates)) {
    const [lng, lat] = g.coordinates;
    if (typeof lng === "number" && typeof lat === "number") return [lng, lat];
  }
  return null;
}

/**
 * Pure helpers for the `bbox=west,south,east,north` URL parameter.
 *
 * Pulled out of `SearchMap` so they're unit-testable in isolation
 * and so the map component itself can stay focused on side effects.
 *
 * Format throughout: GeoJSON-style "lng,lat" pairs in
 * `west,south,east,north` order — same order Mapbox uses for
 * `LngLatBounds.toArray().flat()` and the same the API expects.
 */

export interface Bbox {
  west: number;
  south: number;
  east: number;
  north: number;
}

/**
 * Clamp to legal geographic ranges. Mapbox can hand us bounds with
 * `lat > 90` or `lng > 180` when the user zooms way out (the
 * projection extends past the poles and wraps the meridian). The
 * API rejects those with 400 — clamp them here.
 *
 * Returns `null` when the clamped bbox has zero area (e.g. the
 * caller's input was already degenerate); callers should skip the
 * refetch in that case.
 */
export function clampBbox(input: Bbox): Bbox | null {
  const west = Math.max(-180, Math.min(180, input.west));
  const east = Math.max(-180, Math.min(180, input.east));
  const south = Math.max(-90, Math.min(90, input.south));
  const north = Math.max(-90, Math.min(90, input.north));
  if (west >= east || south >= north) return null;
  return { west, south, east, north };
}

/**
 * Format a bbox for the URL / API param. 4 decimals ≈ 11m precision
 * at the equator — plenty for what's effectively a viewport filter,
 * and short enough that the URL stays readable.
 */
export function bboxToString(b: Bbox): string {
  return [b.west, b.south, b.east, b.north]
    .map((n) => n.toFixed(4))
    .join(",");
}

/**
 * Parse a `bbox` URL/API string. Returns `null` on any malformed
 * input — caller decides whether to ignore or surface a banner.
 * Does NOT clamp; callers that want to fetch should clamp first.
 */
export function parseBboxString(raw: string): Bbox | null {
  const parts = raw.split(",").map(Number);
  if (parts.length !== 4 || !parts.every(Number.isFinite)) return null;
  const [west, south, east, north] = parts;
  return { west, south, east, north };
}

/**
 * Are two bboxes approximately equal? Tolerance defaults to
 * `0.01°` (~1.1 km at the equator) — wide enough to absorb the
 * round-trip rounding that happens when Mapbox's `fitBounds`
 * settles slightly off the requested target (projection +
 * fractional zoom + padding), but tight enough that a real
 * navigation (city pivot, Near me) is still detected as a change.
 *
 * Used by `useUrlBboxFitBounds` to break the feedback loop where
 * a pan writes a bbox to the URL, `useSearchParams` re-reads it
 * (Next 15 makes `replaceState` reactive), and the resulting
 * `fitBounds` emits another `moveend` that writes a slightly-
 * different bbox, ad nauseam.
 */
export function bboxesApproxEqual(
  a: Bbox,
  b: Bbox,
  tolerance = 0.01
): boolean {
  return (
    Math.abs(a.west - b.west) < tolerance &&
    Math.abs(a.east - b.east) < tolerance &&
    Math.abs(a.south - b.south) < tolerance &&
    Math.abs(a.north - b.north) < tolerance
  );
}

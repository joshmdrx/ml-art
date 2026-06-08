/**
 * URL builder for the /search surface.
 *
 * Single helper so the filter-pill clear links, the city-pivot strip,
 * the view-toggle, and the disconnect-explainer don't each reimplement
 * "copy URLSearchParams, drop these keys, set these keys, re-serialize".
 * Keeping it in one place means changes (e.g. a new param) land once.
 */

type SearchParamsLike = Record<string, string | string[] | undefined>;

interface SearchHrefChanges {
  /** Keys to remove from `base` before serialising. */
  drop?: readonly string[];
  /** Keys to set; override `base` if present. */
  set?: Record<string, string>;
}

/**
 * Build a `/search` href from existing params plus mutations.
 *
 * - Array-typed values in `base` are flattened to the first element
 *   (this surface only uses single-value params; the array shape is
 *   imposed by Next.js's `searchParams` typing).
 * - Empty / null / undefined values are skipped.
 * - `drop` always wins; `set` overrides `base`.
 */
export function searchHref(
  base: SearchParamsLike,
  changes: SearchHrefChanges = {},
): string {
  const { drop = [], set = {} } = changes;
  const dropped = new Set(drop);
  const usp = new URLSearchParams();
  for (const [k, v] of Object.entries(base)) {
    if (dropped.has(k)) continue;
    if (k in set) continue;
    const value = Array.isArray(v) ? v[0] : v;
    if (typeof value === "string" && value.length > 0) usp.set(k, value);
  }
  for (const [k, v] of Object.entries(set)) {
    if (dropped.has(k)) continue;
    usp.set(k, v);
  }
  const qs = usp.toString();
  return `/search${qs ? `?${qs}` : ""}`;
}

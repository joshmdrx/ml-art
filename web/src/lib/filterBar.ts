/**
 * Pure helpers for the `FilterBar` component. Extracted so the URL-rewrite
 * logic is unit-testable (Vitest) without rendering React or simulating
 * `useRouter`.
 */

/** The set of pills the FilterBar knows how to render. */
export type FilterKind = "medium" | "price" | "availability" | "location" | "size";

/**
 * T-070 — physical-size bands over the longest side of an artwork's
 * `dimensions` (cm). Bounds mirror the backend's `search::handle`
 * clause so the active-token round-trip is exact.
 */
export interface SizeBand {
  /** Display label. */
  label: string;
  /** URL token. Single letter to keep `/search?size=s` clean. */
  token: "s" | "m" | "l";
}

export const SIZE_BANDS: SizeBand[] = [
  { label: "Small (≤ 40 cm)", token: "s" },
  { label: "Medium (41–100 cm)", token: "m" },
  { label: "Large (> 100 cm)", token: "l" },
];

/** Mapping from a `price` selection to the underlying `price_min`/`price_max`
 * URL params. `min` and `max` are in **canonical-GBP minor units** to match
 * the API contract — the search filter compares against `price_gbp_cents`
 * (T-080). Artworks priced in other currencies (USD/EUR/…) are converted
 * server-side; the bucket boundaries here are pence amounts. */
export interface PriceBucket {
  /** Display label, e.g. "Under £500" — also stored in the URL as `price=u500`. */
  label: string;
  /** Stable URL token — slug-style so it round-trips cleanly. */
  token: string;
  min?: number;
  max?: number;
}

/** Curated price buckets. Tweak ranges as we learn what artists list.
 *  Anchored on GBP after T-080 — UK-artist focus. */
export const PRICE_BUCKETS: PriceBucket[] = [
  { label: "Under £500", token: "u500", max: 50_000 },
  { label: "£500 – £2,000", token: "500-2k", min: 50_000, max: 200_000 },
  { label: "£2,000 – £10,000", token: "2k-10k", min: 200_000, max: 1_000_000 },
  { label: "£10,000+", token: "10kplus", min: 1_000_000 },
];

/** Availability enum values the API accepts (matches the CHECK constraint
 * in `db/migrations/0002_artworks.sql`). */
export const AVAILABILITY_OPTIONS: ReadonlyArray<{ label: string; value: string }> = [
  { label: "Available", value: "available" },
  { label: "Inquire", value: "inquire" },
  { label: "Sold", value: "sold" },
  { label: "Not for sale", value: "not_for_sale" },
];

// T-073 — medium filter now uses the canonical taxonomy codes from
// `lib/medium.ts` (MEDIUM_CATEGORIES). The old MEDIUM_OPTIONS list of
// art-movement names (Impressionism, Cubism…) is gone — those never
// matched what artists actually type, and post-T-073 the server-side
// SQL filter is on `medium_category` anyway.
export { MEDIUM_CATEGORIES } from "@/lib/medium";

/** Parse `?medium=painting,print` into an array of canonical codes.
 * Drops unknown tokens silently (matches the server's
 * `parse_medium_query` tolerance — bookmarked URLs with renamed
 * categories still surface what survives). */
export function parseMediumParam(raw: string | null | undefined): string[] {
  if (!raw) return [];
  // Lazy import to avoid a circular reference at module init.
  // eslint-disable-next-line @typescript-eslint/no-require-imports
  const { isMediumCategory } = require("@/lib/medium") as typeof import("@/lib/medium");
  return raw
    .split(",")
    .map((s) => s.trim())
    .filter((s) => s.length > 0 && isMediumCategory(s));
}

/** Build the `?medium=` value from a list of category codes.
 * Returns `null` for empty (so the FilterBar's
 * `applyFilterParam({medium: null})` clears the URL key). */
export function buildMediumParam(codes: readonly string[]): string | null {
  return codes.length === 0 ? null : codes.join(",");
}

/**
 * Build a new URL search string from `current`, applying a single-key
 * `update`. Setting a value to `null` or `""` removes the key. Used by
 * the FilterBar to compute `router.push(basePath + "?" + result)`.
 */
export function applyFilterParam(
  current: URLSearchParams | ReadonlyMap<string, string>,
  update: Record<string, string | null | undefined>
): string {
  const usp =
    current instanceof URLSearchParams
      ? new URLSearchParams(current)
      : new URLSearchParams([...current.entries()]);
  for (const [k, v] of Object.entries(update)) {
    if (v === null || v === undefined || v === "") {
      usp.delete(k);
    } else {
      usp.set(k, v);
    }
  }
  // Cleaner URLs: drop the leading `?` so callers can do `${path}?${result}`
  // without doubling up when result is empty.
  return usp.toString();
}

/**
 * Convert a price bucket token (URL form) → API params (`price_min`, `price_max`).
 * Returns `null` if no token or token is unknown.
 */
export function priceParamsFromToken(
  token: string | null | undefined
): { price_min?: number; price_max?: number } | null {
  if (!token) return null;
  const bucket = PRICE_BUCKETS.find((b) => b.token === token);
  if (!bucket) return null;
  return { price_min: bucket.min, price_max: bucket.max };
}

/**
 * Inverse: given the current `price_min`/`price_max` on the URL, identify
 * which bucket (if any) is currently selected. Returns the token, or
 * `undefined` if the values don't line up with any bucket.
 */
export function bucketTokenFromPriceParams(
  priceMin: number | undefined,
  priceMax: number | undefined
): string | undefined {
  return PRICE_BUCKETS.find(
    (b) => (b.min ?? undefined) === priceMin && (b.max ?? undefined) === priceMax
  )?.token;
}

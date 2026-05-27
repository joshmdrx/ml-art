/**
 * Pure helpers for the `FilterBar` component. Extracted so the URL-rewrite
 * logic is unit-testable (Vitest) without rendering React or simulating
 * `useRouter`.
 */

/** The set of pills the FilterBar knows how to render. */
export type FilterKind = "medium" | "price" | "availability" | "location";

/** Mapping from a `price` selection to the underlying `price_min`/`price_max`
 * URL params. `min` and `max` are in **cents** to match the API contract. */
export interface PriceBucket {
  /** Display label, e.g. "Under $500" — also stored in the URL as `price=under-500`. */
  label: string;
  /** Stable URL token — slug-style so it round-trips cleanly. */
  token: string;
  min?: number;
  max?: number;
}

/** Curated price buckets. Tweak ranges as we learn what artists list. */
export const PRICE_BUCKETS: PriceBucket[] = [
  { label: "Under $500", token: "u500", max: 50_000 },
  { label: "$500 – $2,000", token: "500-2k", min: 50_000, max: 200_000 },
  { label: "$2,000 – $10,000", token: "2k-10k", min: 200_000, max: 1_000_000 },
  { label: "$10,000+", token: "10kplus", min: 1_000_000 },
];

/** Availability enum values the API accepts (matches the CHECK constraint
 * in `db/migrations/0002_artworks.sql`). */
export const AVAILABILITY_OPTIONS: ReadonlyArray<{ label: string; value: string }> = [
  { label: "Available", value: "available" },
  { label: "Inquire", value: "inquire" },
  { label: "Sold", value: "sold" },
  { label: "Not for sale", value: "not_for_sale" },
];

/**
 * Curated medium list. The seed corpus is the WikiArt sample so values are
 * art-historical styles, not material descriptors. When real artists onboard
 * with mediums like "oil on canvas", this list should be replaced by a
 * server-fed aggregation — see `T-017` (facet counts) for the same query
 * shape; that work and this list become the same change.
 */
export const MEDIUM_OPTIONS: ReadonlyArray<string> = [
  "Impressionism",
  "Post Impressionism",
  "Expressionism",
  "Cubism",
  "Realism",
  "Romanticism",
  "Baroque",
  "Pop Art",
  "Minimalism",
  "Abstract Expressionism",
  "Color Field Painting",
  "Ukiyo E",
];

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

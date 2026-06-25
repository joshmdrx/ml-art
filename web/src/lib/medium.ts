/**
 * Canonical medium taxonomy (T-073) — web client mirror of
 * `core::media::CATEGORIES`.
 *
 * Used by:
 *   - the studio modal category select
 *   - the search FilterBar medium multi-select
 *   - the public + studio artwork display (combiner for "Painting · Oil on linen")
 *
 * Keep this list in lockstep with `api/crates/core/src/media.rs`. The
 * server-side CHECK constraint is the truth — the client validates
 * against the same list to give field-anchored errors before the
 * round-trip, but the server rejects anything the client lets through.
 *
 * Display strings: snake_case codes ↔ Title Case labels. `mixed_media`
 * → "Mixed media" (sentence case, not "Mixed Media") because that's
 * how the existing copy reads ("Listed under Josh Matthews", etc.).
 */

export const MEDIUM_CATEGORIES = [
  "painting",
  "drawing",
  "photography",
  "print",
  "sculpture",
  "mixed_media",
  "collage",
  "textile",
  "ceramic",
  "digital",
  "other",
] as const;

export type MediumCategory = (typeof MEDIUM_CATEGORIES)[number];

const LABELS: Record<MediumCategory, string> = {
  painting: "Painting",
  drawing: "Drawing",
  photography: "Photography",
  print: "Print",
  sculpture: "Sculpture",
  mixed_media: "Mixed media",
  collage: "Collage",
  textile: "Textile",
  ceramic: "Ceramic",
  digital: "Digital",
  other: "Other",
};

/** Title Case label for a canonical code. Returns the raw code as a
 * fallback if some unknown value slips through (e.g. an older API
 * response after a category gets renamed). */
export function mediumLabel(code: MediumCategory | string | null | undefined): string {
  if (!code) return "";
  return (LABELS as Record<string, string>)[code] ?? code;
}

/** Whether `code` is a known canonical category. Used by the
 * FilterBar to drop bookmarked-URL tokens that no longer exist. */
export function isMediumCategory(code: string): code is MediumCategory {
  return (MEDIUM_CATEGORIES as readonly string[]).includes(code);
}

/** Combiner for "Painting · Oil on linen" display strings.
 *
 *   - Both present  → "Painting · Oil on linen"
 *   - Only category → "Painting"
 *   - Only medium   → "Oil on linen"   (legacy / not-yet-categorised)
 *   - Neither       → "" (caller decides whether to render anything)
 */
export function formatMedium(
  category: MediumCategory | string | null | undefined,
  medium: string | null | undefined,
): string {
  const cat = category ? mediumLabel(category) : "";
  const mat = medium?.trim() ?? "";
  if (cat && mat) return `${cat} · ${mat}`;
  return cat || mat;
}

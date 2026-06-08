import Link from "next/link";

/**
 * "Showing X — Clear filter" affordance rendered above the map.
 *
 * Used for the artist filter (T-041) and the city/location filter
 * (T-045 L4). Kept generic so future filters (medium, era, etc.)
 * can slot in without another copy of the JSX.
 *
 * Display formatting (slug → "Title Case", etc.) is the caller's
 * responsibility — this component just renders the pill chrome.
 */
export function FilterPill({
  label,
  value,
  clearHref,
}: {
  /** Lead-in copy, e.g. "Showing where to see". No trailing space. */
  label: string;
  /** Already-formatted display value. */
  value: string;
  /** Where the "Clear filter" link navigates. Usually the current
   * URL minus the param this pill represents. */
  clearHref: string;
}) {
  return (
    <div
      className="mb-4 inline-flex items-center gap-3 text-sm bg-surface border border-border px-3 py-1.5"
      role="status"
    >
      <span>
        {label} <strong>{value}</strong>
      </span>
      <Link
        href={clearHref}
        className="text-muted underline hover:text-foreground"
      >
        Clear filter
      </Link>
    </div>
  );
}

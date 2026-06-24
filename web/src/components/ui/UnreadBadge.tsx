/**
 * Small unread-count badge (T-074).
 *
 * Rendered next to the TopNav "Studio" link when the signed-in artist
 * has deliverable + unread inquiries. Tiny, foreground-coloured (not
 * red — unread inquiries are good news, not an alarm), capped at "9+"
 * so it never grows scary.
 *
 *   <UnreadBadge count={3} label="3 unread inquiries" />
 *
 * Returns `null` for count ≤ 0 so call sites stay compact (same
 * contract as `<FieldError>`). The `label` prop is the SR-only string
 * — visible badge has just the number, but screen readers + power users
 * get the disambiguating text.
 *
 * Keep this component dumb: no fetching, no state, just `count → DOM`.
 * The count comes from `/v1/studio/me`, server-rendered on every page
 * nav. See `decisions.md` 2026-06-23 — T-074 surface decisions.
 */
export function UnreadBadge({
  count,
  label,
}: {
  count: number;
  /** SR-only description; the visible number alone isn't enough
   * context for a screen reader on the parent link. */
  label: string;
}) {
  if (count <= 0) return null;
  const display = count > 9 ? "9+" : String(count);
  return (
    <span
      aria-label={label}
      className="ml-1.5 inline-flex items-center justify-center min-w-[1.25rem] h-5 px-1 text-[10px] leading-none font-medium bg-foreground text-background rounded-full tabular-nums"
    >
      <span aria-hidden="true">{display}</span>
    </span>
  );
}

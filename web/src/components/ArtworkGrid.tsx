import type { ArtworkSummary } from "@/lib/api";
import { ArtworkCard } from "./ArtworkCard";

interface Props {
  items: ArtworkSummary[];
  /**
   * Layout density. `default` is the full-page grid (2/3/4 cols by
   * breakpoint). `compact` is the split-view side-panel grid —
   * narrower container, so 1 col on small phones, 2 on everything
   * else. Cards themselves are identical between the two; only the
   * track count changes.
   */
  density?: "default" | "compact";
}

/**
 * Responsive grid. Two density modes — see `Props.density`.
 */
export function ArtworkGrid({ items, density = "default" }: Props) {
  const className =
    density === "compact"
      ? "grid grid-cols-1 sm:grid-cols-2 gap-4"
      : "grid grid-cols-2 md:grid-cols-3 lg:grid-cols-4 gap-6 md:gap-8";
  return (
    <div className={className}>
      {items.map((a) => (
        <ArtworkCard key={a.id} artwork={a} />
      ))}
    </div>
  );
}

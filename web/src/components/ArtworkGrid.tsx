import type { ArtworkSummary } from "@/lib/api";
import { ArtworkCard } from "./ArtworkCard";

/**
 * Responsive grid. Mobile 2 cols, tablet 3, desktop 4 (per spec).
 */
export function ArtworkGrid({ items }: { items: ArtworkSummary[] }) {
  return (
    <div className="grid grid-cols-2 md:grid-cols-3 lg:grid-cols-4 gap-6 md:gap-8">
      {items.map((a) => (
        <ArtworkCard key={a.id} artwork={a} />
      ))}
    </div>
  );
}

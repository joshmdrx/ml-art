"use client";

/**
 * Side panel of artwork cards for the `/search?map=1` split view
 * (T-045 L2 + L3). Each card listens for:
 *
 *   - **hover** — lifts the artist's slug to the parent
 *     `onHighlightArtist`; both the card and the matching map pin(s)
 *     react via shared state.
 *   - **click** — calls `onFocusArtist`, which the parent uses to
 *     trigger a map `flyTo` + opens the pin's popup. The card itself
 *     does NOT navigate to the artwork detail page in this mode —
 *     navigation happens from the popup's "View portfolio →" link.
 *
 * Stays presentation-only: the actual state lives in the parent
 * SplitView so SearchMap can read it too. Visual treatment:
 * inset shadow on the highlighted card so the indicator never
 * overflows the scroll container (which previously clipped a
 * ring-offset on cards at the panel's left edge).
 */

import type { ArtworkSummary } from "@/lib/api";
import { formatPrice } from "@/lib/format";

interface Props {
  items: ArtworkSummary[];
  /** Current highlighted artist (from sibling state). Cards belonging
   * to this artist render with an inset border. */
  highlightedArtistSlug: string | null;
  /** Called on mouseenter (with the artist's slug) and mouseleave
   * (with null). The parent writes the state. */
  onHighlightArtist: (slug: string | null) => void;
  /** Called on card click. The parent uses this to fly the map to
   * the artist's first pin and open its popup. */
  onFocusArtist: (slug: string) => void;
  /** Page-size cap from the search page. Used to show "N+ works"
   * when we know the result was truncated. */
  pageLimit: number;
}

export function SearchSidePanel({
  items,
  highlightedArtistSlug,
  onHighlightArtist,
  onFocusArtist,
  pageLimit,
}: Props) {
  return (
    <>
      <p className="mb-3 text-xs uppercase tracking-wider text-muted">
        {items.length}
        {items.length >= pageLimit ? "+" : ""} work
        {items.length === 1 ? "" : "s"}
      </p>
      <div className="grid grid-cols-1 sm:grid-cols-2 gap-4">
        {items.map((a) => (
          <SidePanelCard
            key={a.id}
            artwork={a}
            isHighlighted={highlightedArtistSlug === a.artist_slug}
            onMouseEnter={() => onHighlightArtist(a.artist_slug)}
            onMouseLeave={() => onHighlightArtist(null)}
            onClick={() => onFocusArtist(a.artist_slug)}
          />
        ))}
      </div>
    </>
  );
}

/**
 * Inlined card variant. We don't reuse `<ArtworkCard>` here because
 * the side-panel card is a click target that focuses the map — it
 * intentionally does NOT navigate to /artworks/:id. ArtworkCard's
 * nested <Link>s would conflict; using a single <button> lets us
 * fully own the click semantics.
 */
function SidePanelCard({
  artwork,
  isHighlighted,
  onMouseEnter,
  onMouseLeave,
  onClick,
}: {
  artwork: ArtworkSummary;
  isHighlighted: boolean;
  onMouseEnter: () => void;
  onMouseLeave: () => void;
  onClick: () => void;
}) {
  const price = formatPrice(artwork.price_cents, artwork.currency);
  const altText = artwork.title
    ? `${artwork.title} by ${artwork.artist_name}`
    : `Untitled by ${artwork.artist_name}`;

  return (
    <button
      type="button"
      data-highlighted={isHighlighted}
      onMouseEnter={onMouseEnter}
      onMouseLeave={onMouseLeave}
      onClick={onClick}
      aria-label={`Show ${altText} on the map`}
      className={
        "group text-left w-full transition-shadow " +
        // Inset shadow — stays inside the card's bounding box, so
        // the scrollable side panel can clip flush at the edge
        // without lopping a ring off. 2px black-ish; the foreground
        // colour is the design system's near-black.
        "data-[highlighted=true]:shadow-[inset_0_0_0_2px_var(--color-foreground,#1a1a1a)]"
      }
    >
      <div className="bg-surface border border-border overflow-hidden">
        {artwork.primary_image_url ? (
          // eslint-disable-next-line @next/next/no-img-element
          <img
            src={artwork.primary_image_url}
            alt={altText}
            loading="lazy"
            className="w-full h-auto block transition-opacity group-hover:opacity-95"
          />
        ) : (
          <div className="aspect-square bg-border" />
        )}
      </div>
      <div className="mt-2 px-0.5">
        <p className="text-sm text-muted truncate">{artwork.artist_name}</p>
        <p className="text-sm font-serif truncate">
          {artwork.title ?? "Untitled"}
        </p>
        {price && <p className="text-xs text-muted mt-0.5">{price}</p>}
      </div>
    </button>
  );
}

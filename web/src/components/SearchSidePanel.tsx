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

import Link from "next/link";

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
  /** Called on card click. The parent uses this to drive the map
   * popup — to a pin for mapped artists, or a center-anchored
   * "Not on the map yet" popup for unmapped ones. The artwork
   * carries enough data to render either case without an extra
   * lookup. */
  onFocusArtist: (artwork: ArtworkSummary) => void;
  /** How many of `items` have at least one pin in the map's current
   * visible set. Drives the "N of M mapped" caption (T-045 L4). */
  mappedCount: number;
  /** Set when there are items but none of them are mapped (a useless
   * map view). Renders the inline "Back to Works →" link. Replaces
   * the disconnect-explainer's hostile copy. */
  backToWorksHref?: string;
  /** True when more pages exist (cursor pagination, T-037). Drives
   * the caption's "+" suffix and the "Load more" button. */
  hasMore: boolean;
  /** True while a load-more request is in flight. */
  loadingMore: boolean;
  /** Error string from the most recent failed load-more. */
  loadMoreError: string | null;
  /** Click handler for the "Load more" button. */
  onLoadMore: () => void;
}

export function SearchSidePanel({
  items,
  highlightedArtistSlug,
  onHighlightArtist,
  onFocusArtist,
  mappedCount,
  backToWorksHref,
  hasMore,
  loadingMore,
  loadMoreError,
  onLoadMore,
}: Props) {
  return (
    <>
      <ResultsCaption
        total={items.length}
        mapped={mappedCount}
        hitLimit={hasMore}
        backToWorksHref={backToWorksHref}
      />
      <div className="grid grid-cols-1 sm:grid-cols-2 gap-4">
        {items.map((a) => (
          <SidePanelCard
            key={a.id}
            artwork={a}
            isHighlighted={highlightedArtistSlug === a.artist_slug}
            onMouseEnter={() => onHighlightArtist(a.artist_slug)}
            onMouseLeave={() => onHighlightArtist(null)}
            onClick={() => onFocusArtist(a)}
          />
        ))}
      </div>
      {(hasMore || loadMoreError) && (
        <LoadMoreFooter
          hasMore={hasMore}
          loading={loadingMore}
          error={loadMoreError}
          onClick={onLoadMore}
        />
      )}
    </>
  );
}

/**
 * Bottom-of-panel "Load more" button + error surface. Lives here
 * (not in SearchSplitView) so the click target sits inside the
 * scrollable panel — clicking it doesn't make the panel jump.
 */
function LoadMoreFooter({
  hasMore,
  loading,
  error,
  onClick,
}: {
  hasMore: boolean;
  loading: boolean;
  error: string | null;
  onClick: () => void;
}) {
  return (
    <div className="mt-6 mb-2 flex flex-col items-center gap-2 text-sm">
      {hasMore && (
        <button
          type="button"
          onClick={onClick}
          disabled={loading}
          className="border border-border bg-surface px-4 py-2 hover:bg-background disabled:opacity-60 disabled:cursor-progress"
        >
          {loading ? "Loading…" : "Load more"}
        </button>
      )}
      {error && (
        <p role="status" className="text-red-600 text-xs">
          {error}
        </p>
      )}
    </div>
  );
}

/**
 * "N of M mapped" caption. Replaces the old "24+ WORKS" line and
 * the disconnect explainer in one shot — same information, less
 * shouting. The "Back to Works →" link only renders when there are
 * items but none of them are mapped (i.e. the map view is useless
 * right now and the user wants an exit).
 */
function ResultsCaption({
  total,
  mapped,
  hitLimit,
  backToWorksHref,
}: {
  total: number;
  mapped: number;
  hitLimit: boolean;
  backToWorksHref?: string;
}) {
  if (total === 0) return null;
  return (
    // Hidden on mobile: the bottom-sheet's handle in SearchSplitView
    // shows the count instead, so we don't render the caption twice.
    <p className="hidden lg:block mb-3 text-xs uppercase tracking-wider text-muted">
      {mappedCountLabel(mapped, total, hitLimit)}
      {backToWorksHref && (
        <>
          {" — "}
          <Link
            href={backToWorksHref}
            className="underline underline-offset-2 hover:text-foreground normal-case tracking-normal"
          >
            Back to Works →
          </Link>
        </>
      )}
    </p>
  );
}

/**
 * Shared text helper for the "N of M mapped" caption. Used by the
 * desktop ResultsCaption above and the mobile bottom-sheet handle
 * in SearchSplitView — same wording in both places so they read
 * consistently.
 *
 * Note we deliberately never say "All X+ mapped" — the `+` means
 * "the result was truncated, more works exist" and "all" implies
 * completeness. Saying both is a contradiction. The honest form
 * is always `N of M[+] mapped`, even when `N === M`.
 */
export function mappedCountLabel(
  mapped: number,
  total: number,
  hitLimit: boolean,
): string {
  const totalLabel = `${total}${hitLimit ? "+" : ""}`;
  if (mapped === 0) return `None of ${totalLabel} mapped`;
  return `${mapped} of ${totalLabel} mapped`;
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
      data-artwork-id={artwork.id}
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

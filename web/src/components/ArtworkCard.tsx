import Link from "next/link";
// Type-only import is erased at compile time, so `lib/api`'s
// runtime Clerk import doesn't follow into the client bundle.
import type { ArtworkSummary } from "@/lib/api";
// Pure formatter, no server-only deps — safe in client components.
import { formatPrice } from "@/lib/format";

/**
 * Card used in every grid view.
 *
 * The image links to artwork detail; the artist name is a *separate* link to
 * the artist portfolio. Can't nest <a> inside <a>, so they're siblings.
 */
export function ArtworkCard({ artwork }: { artwork: ArtworkSummary }) {
  const price = formatPrice(artwork.price_cents, artwork.currency);
  const altText = artwork.title
    ? `${artwork.title} by ${artwork.artist_name}`
    : `Untitled by ${artwork.artist_name}`;

  return (
    <div className="group">
      <Link
        href={`/artworks/${artwork.id}`}
        aria-label={altText}
        className="block bg-surface border border-border overflow-hidden"
      >
        {artwork.primary_image_url ? (
          // Plain <img> — Next/Image needs a remote-host allow-list, will
          // configure once we lock in CloudFront.
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
      </Link>

      <div className="mt-2 px-0.5">
        <Link
          href={`/artists/${artwork.artist_slug}`}
          className="text-sm text-muted hover:text-foreground truncate block"
        >
          {artwork.artist_name}
        </Link>
        <Link
          href={`/artworks/${artwork.id}`}
          className="text-sm font-serif hover:underline truncate block"
        >
          {artwork.title ?? "Untitled"}
        </Link>
        {price && <div className="text-xs text-muted mt-0.5">{price}</div>}
      </div>
    </div>
  );
}

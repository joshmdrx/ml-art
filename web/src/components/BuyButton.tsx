import Link from "next/link";

/**
 * Buy button on the artwork detail page (M-05). Only rendered when the
 * artwork is `purchasable` (see `ArtworkFull.purchasable`). Routes to the
 * dedicated buy page — the shipping form + order summary live there, and
 * that page gates on sign-in.
 */
export function BuyButton({ artworkId }: { artworkId: string }) {
  return (
    <Link
      href={`/artworks/${encodeURIComponent(artworkId)}/buy`}
      className="w-full py-3 px-4 bg-foreground text-background text-sm text-center hover:bg-foreground/90 transition-colors"
    >
      Buy now
    </Link>
  );
}

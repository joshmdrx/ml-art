import type { Metadata } from "next";
import Link from "next/link";
import { notFound, redirect } from "next/navigation";
import { auth, currentUser } from "@clerk/nextjs/server";
import { TopNav } from "@/components/TopNav";
import { BuyForm } from "@/components/BuyForm";
import { getArtwork, formatPrice } from "@/lib/api";
import { reportError } from "@/lib/reportError";

export const metadata: Metadata = {
  title: "Checkout",
};

type Params = Promise<{ id: string }>;

/**
 * `/artworks/[id]/buy` — shipping form + order summary (M-05).
 *
 *   - Signed-out → /sign-in?redirect_url=/artworks/[id]/buy
 *   - Not purchasable (or not found) → back to the artwork page
 *   - Signed-in + purchasable → summary + BuyForm, which opens Stripe
 *     Checkout via the `startCheckout` server action.
 */
export default async function BuyPage({ params }: { params: Params }) {
  const { id } = await params;

  const { userId } = await auth();
  if (!userId) {
    redirect(
      "/sign-in?redirect_url=" +
        encodeURIComponent(`/artworks/${id}/buy`)
    );
  }

  const artwork = await getArtwork(id).catch((e) => {
    reportError(e, { surface: "buy-page", id });
    return null;
  });
  if (!artwork) notFound();

  // Server-side re-gate: the Buy button only shows when purchasable, but
  // a deep link shouldn't land on a dead form. The checkout endpoint
  // re-checks too — this is just for a clean redirect.
  if (!artwork.purchasable) {
    redirect(`/artworks/${id}`);
  }

  const user = await currentUser();
  const defaultName =
    user?.fullName ??
    [user?.firstName, user?.lastName].filter(Boolean).join(" ") ??
    "";

  const image = artwork.images.find((i) => i.is_primary) ?? artwork.images[0];
  const price = formatPrice(artwork.price_cents, artwork.currency);

  return (
    <>
      <TopNav />
      <main className="flex-1 mx-auto w-full max-w-2xl px-6 py-10 md:py-12">
        <Link
          href={`/artworks/${id}`}
          className="text-xs text-muted hover:text-foreground"
        >
          ← Back to artwork
        </Link>

        <h1 className="font-serif text-3xl tracking-tight mt-4">Checkout</h1>

        {/* Order summary */}
        <section className="mt-8 flex gap-4 border border-border bg-surface p-4">
          {image && (
            /* eslint-disable-next-line @next/next/no-img-element */
            <img
              src={image.url}
              alt={artwork.title ?? "Artwork"}
              className="w-24 h-24 shrink-0 object-cover bg-background"
            />
          )}
          <div className="min-w-0">
            <p className="font-serif text-lg leading-tight">
              {artwork.title ?? "Untitled"}
            </p>
            <p className="text-sm text-muted mt-1">
              {artwork.artist.display_name}
            </p>
            <p className="text-sm mt-2">{price ?? "—"}</p>
          </div>
        </section>

        <p className="mt-3 text-xs text-muted">
          Shipping is arranged directly with the artist. You&apos;ll pay
          securely via Stripe; Wander never sees your card details.
        </p>

        <BuyForm artworkId={artwork.id} defaultName={defaultName} />
      </main>
    </>
  );
}

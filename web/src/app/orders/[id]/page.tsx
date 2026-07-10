import type { Metadata } from "next";
import Link from "next/link";
import { notFound, redirect } from "next/navigation";
import { auth } from "@clerk/nextjs/server";
import { TopNav } from "@/components/TopNav";
import { getOrder, formatPrice, type OrderStatus } from "@/lib/api";
import { reportError } from "@/lib/reportError";

export const metadata: Metadata = {
  title: "Your order",
};

type Params = Promise<{ id: string }>;

/**
 * `/orders/[id]` — post-checkout order confirmation (M-05).
 *
 * Stripe redirects here on success, but the order may still be `pending`
 * for a beat until `checkout.session.completed` lands (webhook is async),
 * so the copy is status-aware rather than assuming "paid".
 */
export default async function OrderPage({ params }: { params: Params }) {
  const { id } = await params;

  const { userId } = await auth();
  if (!userId) {
    redirect("/sign-in?redirect_url=" + encodeURIComponent(`/orders/${id}`));
  }

  const order = await getOrder(id).catch((e) => {
    reportError(e, { surface: "order-detail", id });
    return null;
  });
  if (!order) notFound();

  const { headline, sub } = copyForStatus(order.status);
  const amount = formatPrice(order.amount_cents_gbp, "GBP");
  const addr = order.shipping_address;

  return (
    <>
      <TopNav />
      <main className="flex-1 mx-auto w-full max-w-2xl px-6 py-10 md:py-12">
        <h1 className="font-serif text-3xl tracking-tight">{headline}</h1>
        <p className="mt-2 text-sm text-muted">{sub}</p>

        {/* Artwork */}
        <section className="mt-8 flex gap-4 border border-border bg-surface p-4">
          {order.artwork.image_url && (
            /* eslint-disable-next-line @next/next/no-img-element */
            <img
              src={order.artwork.image_url}
              alt={order.artwork.title ?? "Artwork"}
              className="w-24 h-24 shrink-0 object-cover bg-background"
            />
          )}
          <div className="min-w-0">
            <Link
              href={`/artworks/${order.artwork.id}`}
              className="font-serif text-lg leading-tight underline underline-offset-2 hover:no-underline"
            >
              {order.artwork.title ?? "Untitled"}
            </Link>
            <p className="text-sm text-muted mt-1">
              {order.artwork.artist_name}
            </p>
            <p className="text-sm mt-2">{amount ?? "—"}</p>
          </div>
        </section>

        {/* Shipping */}
        <section className="mt-6 text-sm">
          <h2 className="font-serif text-lg mb-2">Shipping to</h2>
          <address className="not-italic text-muted leading-relaxed">
            {addr.name}
            <br />
            {addr.line1}
            {addr.line2 && (
              <>
                <br />
                {addr.line2}
              </>
            )}
            <br />
            {addr.city}, {addr.postal_code}
            <br />
            {addr.country}
          </address>
        </section>

        <div className="mt-10 flex gap-3">
          <Link
            href="/orders"
            className="px-4 py-2 text-sm border border-border hover:bg-background"
          >
            View all orders
          </Link>
          <Link
            href="/search"
            className="px-4 py-2 text-sm bg-foreground text-background hover:bg-foreground/90"
          >
            Keep exploring
          </Link>
        </div>
      </main>
    </>
  );
}

function copyForStatus(status: OrderStatus): { headline: string; sub: string } {
  switch (status) {
    case "pending":
      return {
        headline: "Confirming your payment…",
        sub: "This usually takes a few seconds. Refresh in a moment — we'll email you once it's confirmed.",
      };
    case "paid":
      return {
        headline: "Order confirmed",
        sub: "Thank you — the artist has been notified and will arrange shipping. You'll get tracking by email.",
      };
    case "shipped":
      return {
        headline: "On its way",
        sub: "The artist has shipped your order. Tracking details are in your email.",
      };
    case "delivered":
      return { headline: "Delivered", sub: "We hope you love it." };
    case "cancelled":
      return {
        headline: "Order cancelled",
        sub: "This order was cancelled. Any payment has been refunded.",
      };
    case "refunded":
      return {
        headline: "Order refunded",
        sub: "This order was refunded. The amount should appear on your statement within a few days.",
      };
    case "disputed":
      return {
        headline: "Order under review",
        sub: "This order is being reviewed. We'll be in touch by email.",
      };
    default:
      return { headline: "Your order", sub: "" };
  }
}

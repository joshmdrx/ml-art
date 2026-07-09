import type { Metadata } from "next";
import Link from "next/link";
import { notFound, redirect } from "next/navigation";
import { auth } from "@clerk/nextjs/server";
import { MarkShippedForm } from "@/components/studio/MarkShippedForm";
import { getStudioOrder, formatPrice } from "@/lib/api";
import { reportError } from "@/lib/reportError";

export const metadata: Metadata = { title: "Order" };

type Params = Promise<{ id: string }>;

/**
 * `/studio/orders/[id]` — order detail + fulfilment (M-06). Shows the
 * buyer + shipping address and, when the order is `paid`, the
 * mark-shipped form. Everything is scoped to the calling artist by the
 * API (404 for someone else's order).
 */
export default async function StudioOrderPage({ params }: { params: Params }) {
  const { id } = await params;

  const { userId } = await auth();
  if (!userId) {
    redirect(
      "/sign-in?redirect_url=" + encodeURIComponent(`/studio/orders/${id}`)
    );
  }

  const order = await getStudioOrder(id).catch((e) => {
    reportError(e, { surface: "studio-order-detail", id });
    return null;
  });
  if (!order) notFound();

  const addr = order.shipping_address;
  const payout = order.amount_cents_gbp - order.commission_cents_gbp;

  return (
    <div className="flex-1 px-6 py-8 lg:py-10 max-w-2xl">
      <Link
        href="/studio/orders"
        className="text-xs text-muted hover:text-foreground"
      >
        ← All orders
      </Link>

      <div className="mt-4 flex items-center justify-between gap-4">
        <h1 className="font-serif text-2xl tracking-tight">
          {order.artwork.title ?? "Untitled"}
        </h1>
        <span className="text-xs px-2 py-1 border border-border rounded-full capitalize text-muted shrink-0">
          {order.status}
        </span>
      </div>

      {/* Money — the artist sees their payout, not the gross. */}
      <dl className="mt-6 text-sm space-y-2">
        <Row label="Sale price" value={formatPrice(order.amount_cents_gbp, "GBP") ?? "—"} />
        <Row
          label="Wander commission"
          value={"−" + (formatPrice(order.commission_cents_gbp, "GBP") ?? "—")}
        />
        <Row
          label="Your payout"
          value={formatPrice(order.payout_cents_gbp ?? payout, "GBP") ?? "—"}
          strong
        />
      </dl>

      {/* Buyer + shipping */}
      <section className="mt-8 text-sm">
        <h2 className="font-serif text-lg mb-2">Ship to</h2>
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
        {order.buyer_email && (
          <p className="mt-2 text-xs text-muted">
            Buyer: {order.buyer_name} · {order.buyer_email}
          </p>
        )}
      </section>

      {/* Fulfilment */}
      {order.status === "paid" && (
        <section className="mt-8">
          <h2 className="font-serif text-lg mb-3">Mark as shipped</h2>
          <MarkShippedForm orderId={order.id} />
        </section>
      )}

      {order.tracking_number && (
        <section className="mt-8 text-sm">
          <h2 className="font-serif text-lg mb-2">Shipped</h2>
          <p className="text-muted">
            {order.tracking_carrier} · {order.tracking_number}
          </p>
        </section>
      )}
    </div>
  );
}

function Row({
  label,
  value,
  strong,
}: {
  label: string;
  value: string;
  strong?: boolean;
}) {
  return (
    <div className="flex items-baseline justify-between gap-4">
      <dt className="text-muted">{label}</dt>
      <dd className={strong ? "font-medium" : ""}>{value}</dd>
    </div>
  );
}

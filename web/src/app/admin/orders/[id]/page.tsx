import type { Metadata } from "next";
import Link from "next/link";
import { notFound } from "next/navigation";
import { TopNav } from "@/components/TopNav";
import { RefundButton } from "@/components/admin/RefundButton";
import { getAdminOrder, getMe, formatPrice } from "@/lib/api";

export const metadata: Metadata = {
  title: "Admin · Order — Wander",
  robots: { index: false, follow: false },
};

type Params = Promise<{ id: string }>;

const REFUNDABLE = new Set(["paid", "shipped", "delivered", "disputed"]);

/** M-08 — `/admin/orders/[id]` detail + refund + dispute banner. */
export default async function AdminOrderPage({ params }: { params: Params }) {
  const { id } = await params;

  const me = await getMe().catch(() => null);
  if (!me?.is_admin) notFound();

  const order = await getAdminOrder(id).catch(() => null);
  if (!order) notFound();

  const addr = order.shipping_address;
  const pi = order.stripe_payment_intent_id;

  return (
    <>
      <TopNav />
      <main className="flex-1 mx-auto w-full max-w-2xl px-6 py-12">
        <Link
          href="/admin/orders"
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

        {order.status === "disputed" && (
          <div className="mt-4 border border-border bg-surface p-4 text-sm">
            <p className="font-medium">⚠ Chargeback filed</p>
            <p className="text-muted mt-1">
              A dispute was opened on this order. Submit evidence in the
              Stripe dashboard — we don&apos;t handle evidence here.
            </p>
          </div>
        )}

        {order.refund_reason && (
          <p className="mt-4 text-sm text-muted">
            Refund reason on file: <strong>{order.refund_reason}</strong>
          </p>
        )}

        <dl className="mt-6 text-sm space-y-2">
          <Row label="Sale price" value={formatPrice(order.amount_cents_gbp, "GBP") ?? "—"} />
          <Row label="Commission" value={formatPrice(order.commission_cents_gbp, "GBP") ?? "—"} />
          <Row
            label="Artist payout"
            value={
              formatPrice(
                order.payout_cents_gbp ??
                  order.amount_cents_gbp - order.commission_cents_gbp,
                "GBP"
              ) ?? "—"
            }
          />
          <Row label="Artist" value={order.artist_name} />
          <Row label="Buyer" value={`${order.buyer_name ?? "—"} · ${order.buyer_email ?? "—"}`} />
        </dl>

        <section className="mt-6 text-sm">
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
          {order.tracking_number && (
            <p className="mt-2 text-xs text-muted">
              Shipped: {order.tracking_carrier} · {order.tracking_number}
            </p>
          )}
        </section>

        <div className="mt-8 flex flex-wrap items-center gap-3">
          {pi && (
            <a
              href={`https://dashboard.stripe.com/test/payments/${pi}`}
              target="_blank"
              rel="noopener noreferrer"
              className="px-4 py-2 text-sm border border-border hover:bg-background"
            >
              Open in Stripe ↗
            </a>
          )}
          {REFUNDABLE.has(order.status) && (
            <RefundButton orderId={order.id} />
          )}
        </div>
      </main>
    </>
  );
}

function Row({ label, value }: { label: string; value: string }) {
  return (
    <div className="flex items-baseline justify-between gap-4">
      <dt className="text-muted">{label}</dt>
      <dd>{value}</dd>
    </div>
  );
}

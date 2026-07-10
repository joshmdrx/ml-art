import type { Metadata } from "next";
import Link from "next/link";
import { redirect } from "next/navigation";
import { auth } from "@clerk/nextjs/server";
import {
  listStudioOrders,
  getPayoutStatus,
  formatPrice,
  type StudioOrderSummary,
} from "@/lib/api";
import { reportError } from "@/lib/reportError";

export const metadata: Metadata = { title: "Studio orders" };

/**
 * `/studio/orders` — the artist's sales dashboard (M-06). Lists their
 * orders newest-first; a banner nudges Stripe onboarding when payouts
 * aren't live yet (without it nothing here can ever be `paid`).
 */
export default async function StudioOrdersPage() {
  const { userId } = await auth();
  if (!userId) {
    redirect("/sign-in?redirect_url=" + encodeURIComponent("/studio/orders"));
  }

  const [orders, payout] = await Promise.all([
    listStudioOrders().catch((e) => {
      reportError(e, { surface: "studio-orders" });
      return [] as StudioOrderSummary[];
    }),
    getPayoutStatus().catch(() => null),
  ]);

  return (
    <div className="flex-1 px-6 py-8 lg:py-10">
      <h1 className="font-serif text-3xl tracking-tight">Orders</h1>

      {payout && !payout.charges_enabled && (
        <div className="mt-6 border border-border bg-surface p-4 text-sm">
          <p className="font-medium">Direct sales aren&apos;t live yet.</p>
          <p className="text-muted mt-1">
            Finish setting up payouts to let collectors buy your work
            directly through Wander.
          </p>
          <Link
            href="/studio/settings/payouts"
            className="inline-block mt-3 px-4 py-2 bg-foreground text-background text-sm hover:bg-foreground/90"
          >
            Set up payouts →
          </Link>
        </div>
      )}

      {orders.length === 0 ? (
        <p className="mt-8 text-sm text-muted">
          No orders yet. When a collector buys one of your works, it&apos;ll
          show up here to fulfil.
        </p>
      ) : (
        <ul className="mt-8 divide-y divide-border border-y border-border">
          {orders.map((o) => (
            <li key={o.id}>
              <Link
                href={`/studio/orders/${o.id}`}
                className="flex items-center justify-between gap-4 py-4 hover:bg-background px-2 -mx-2 transition-colors"
              >
                <div className="min-w-0">
                  <p className="text-sm font-medium truncate">
                    {o.artwork_title ?? "Untitled"}
                  </p>
                  <p className="text-xs text-muted mt-0.5">
                    {o.buyer_name ?? "Buyer"} ·{" "}
                    {new Date(o.created_at).toLocaleDateString("en-GB")}
                  </p>
                </div>
                <div className="flex items-center gap-4 shrink-0">
                  <span className="text-sm">
                    {formatPrice(o.amount_cents_gbp, "GBP")}
                  </span>
                  <StatusPill status={o.status} />
                </div>
              </Link>
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}

function StatusPill({ status }: { status: string }) {
  return (
    <span className="text-xs px-2 py-1 border border-border rounded-full capitalize text-muted">
      {status}
    </span>
  );
}

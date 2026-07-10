import type { Metadata } from "next";
import Link from "next/link";
import { redirect } from "next/navigation";
import { auth } from "@clerk/nextjs/server";
import { TopNav } from "@/components/TopNav";
import { listMyOrders, formatPrice } from "@/lib/api";
import { reportError } from "@/lib/reportError";

export const metadata: Metadata = { title: "Your orders" };

/**
 * `/orders` — the buyer's purchase history (M-11). Signed-in only; each
 * row links to the order confirmation/status page.
 */
export default async function OrdersPage() {
  const { userId } = await auth();
  if (!userId) {
    redirect("/sign-in?redirect_url=" + encodeURIComponent("/orders"));
  }

  const orders = await listMyOrders().catch((e) => {
    reportError(e, { surface: "orders-list" });
    return [];
  });

  return (
    <>
      <TopNav />
      <main className="flex-1 mx-auto w-full max-w-2xl px-6 py-10 md:py-12">
        <h1 className="font-serif text-3xl tracking-tight">Your orders</h1>

        {orders.length === 0 ? (
          <p className="mt-8 text-sm text-muted">
            You haven&apos;t bought anything yet.{" "}
            <Link href="/search" className="underline underline-offset-2 hover:no-underline">
              Discover artists →
            </Link>
          </p>
        ) : (
          <ul className="mt-8 divide-y divide-border border-y border-border">
            {orders.map((o) => (
              <li key={o.id}>
                <Link
                  href={`/orders/${o.id}`}
                  className="flex items-center gap-4 py-4 hover:bg-background px-2 -mx-2 transition-colors"
                >
                  {o.artwork.image_url && (
                    /* eslint-disable-next-line @next/next/no-img-element */
                    <img
                      src={o.artwork.image_url}
                      alt={o.artwork.title ?? "Artwork"}
                      className="w-16 h-16 shrink-0 object-cover bg-background"
                    />
                  )}
                  <div className="min-w-0 flex-1">
                    <p className="text-sm font-medium truncate">
                      {o.artwork.title ?? "Untitled"}
                    </p>
                    <p className="text-xs text-muted mt-0.5">
                      {o.artwork.artist_name} ·{" "}
                      {new Date(o.created_at).toLocaleDateString("en-GB")}
                    </p>
                  </div>
                  <div className="flex items-center gap-4 shrink-0">
                    <span className="text-sm">
                      {formatPrice(o.amount_cents_gbp, "GBP")}
                    </span>
                    <span className="text-xs px-2 py-1 border border-border rounded-full capitalize text-muted">
                      {o.status}
                    </span>
                  </div>
                </Link>
              </li>
            ))}
          </ul>
        )}
      </main>
    </>
  );
}

import type { Metadata } from "next";
import Link from "next/link";
import { clsx } from "clsx";
import { TopNav } from "@/components/TopNav";
import { listAdminOrders, getMe, formatPrice } from "@/lib/api";
import { notFound } from "next/navigation";

export const metadata: Metadata = {
  title: "Admin · Orders — Wander",
  robots: { index: false, follow: false },
};

type Search = { status?: string };

const TABS: Array<{ value: string; label: string }> = [
  { value: "all", label: "All" },
  { value: "paid", label: "Paid" },
  { value: "shipped", label: "Shipped" },
  { value: "delivered", label: "Delivered" },
  { value: "disputed", label: "Disputed" },
  { value: "refunded", label: "Refunded" },
];

/** M-08 — `/admin/orders` queue. Filter by status; row → detail + refund. */
export default async function AdminOrdersPage({
  searchParams,
}: {
  searchParams: Promise<Search>;
}) {
  // Defensive gate (the layout also gates; belt + braces).
  const me = await getMe().catch(() => null);
  if (!me?.is_admin) notFound();

  const sp = await searchParams;
  const active = TABS.find((t) => t.value === sp.status)?.value ?? "all";
  const orders = await listAdminOrders(
    active === "all" ? undefined : active
  ).catch(() => []);

  return (
    <>
      <TopNav />
      <main className="flex-1 mx-auto w-full max-w-screen-lg px-6 py-12">
        <header className="mb-6">
          <p className="text-xs text-muted mb-1">Admin</p>
          <h1 className="font-serif text-3xl tracking-tight">Orders</h1>
        </header>

        <nav className="flex gap-1 mb-6 overflow-x-auto">
          {TABS.map((t) => (
            <Link
              key={t.value}
              href={
                t.value === "all"
                  ? "/admin/orders"
                  : `/admin/orders?status=${t.value}`
              }
              className={clsx(
                "px-3 py-1.5 text-sm whitespace-nowrap",
                t.value === active
                  ? "bg-foreground text-background"
                  : "text-muted hover:text-foreground hover:bg-background"
              )}
            >
              {t.label}
            </Link>
          ))}
        </nav>

        {orders.length === 0 ? (
          <p className="text-sm text-muted">No orders.</p>
        ) : (
          <ul className="divide-y divide-border border-y border-border">
            {orders.map((o) => (
              <li key={o.id}>
                <Link
                  href={`/admin/orders/${o.id}`}
                  className="flex items-center justify-between gap-4 py-4 hover:bg-background px-2 -mx-2 transition-colors"
                >
                  <div className="min-w-0">
                    <p className="text-sm font-medium truncate">
                      {o.artwork_title ?? "Untitled"}
                    </p>
                    <p className="text-xs text-muted mt-0.5">
                      {o.artist_name} → {o.buyer_name ?? "Buyer"} ·{" "}
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

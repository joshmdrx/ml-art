import type { Metadata } from "next";
import Link from "next/link";
import { clsx } from "clsx";
import { TopNav } from "@/components/TopNav";
import { AdminVenueRow } from "@/components/admin/AdminVenueRow";
import { listAdminVenues } from "@/lib/api";

export const metadata: Metadata = {
  title: "Admin · Venues — Wander",
  robots: { index: false, follow: false },
};

type Search = { status?: string; cursor?: string };

const TABS: Array<{ value: string; label: string }> = [
  { value: "pending_review", label: "Pending" },
  { value: "active", label: "Active" },
  { value: "paused", label: "Paused" },
  { value: "declined", label: "Declined" },
];

export default async function AdminVenuesPage({
  searchParams,
}: {
  searchParams: Promise<Search>;
}) {
  const sp = await searchParams;
  const status =
    TABS.find((t) => t.value === sp.status)?.value ?? "pending_review";

  const page = await listAdminVenues({ status, cursor: sp.cursor }).catch(
    () => null,
  );
  const items = page?.items ?? [];

  return (
    <>
      <TopNav />
      <main className="flex-1 mx-auto w-full max-w-screen-lg px-6 py-12">
        <header className="mb-6">
          <p className="text-xs text-muted mb-1">
            <Link href="/admin" className="hover:text-foreground">
              ← Admin
            </Link>
          </p>
          <h1 className="font-serif text-3xl tracking-tight">Venues</h1>
        </header>

        <nav className="mb-6 flex flex-wrap gap-2" aria-label="Status filter">
          {TABS.map((t) => (
            <Link
              key={t.value}
              href={`/admin/venues?status=${t.value}`}
              className={clsx(
                "px-3 py-1.5 text-sm border transition-colors",
                t.value === status
                  ? "border-foreground bg-foreground text-background"
                  : "border-border bg-surface hover:bg-background",
              )}
            >
              {t.label}
            </Link>
          ))}
        </nav>

        {items.length === 0 ? (
          <p className="text-sm text-muted">No venues in this queue.</p>
        ) : (
          <ul className="divide-y divide-border border border-border bg-surface">
            {items.map((v) => (
              <AdminVenueRow key={v.id} venue={v} />
            ))}
          </ul>
        )}

        {page?.next_cursor && (
          <div className="mt-6 text-center">
            <Link
              href={`/admin/venues?status=${status}&cursor=${page.next_cursor}`}
              className="text-sm text-muted hover:text-foreground underline underline-offset-2"
            >
              Next page →
            </Link>
          </div>
        )}
      </main>
    </>
  );
}

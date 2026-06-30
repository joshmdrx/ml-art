import type { Metadata } from "next";
import Link from "next/link";
import { clsx } from "clsx";
import { TopNav } from "@/components/TopNav";
import { AdminArtistRow } from "@/components/admin/AdminArtistRow";
import { listAdminArtists } from "@/lib/api";

export const metadata: Metadata = {
  title: "Admin · Artists — Wander",
  robots: { index: false, follow: false },
};

type Search = { status?: string; cursor?: string };

const TABS: Array<{ value: string; label: string }> = [
  { value: "pending", label: "Pending" },
  { value: "active", label: "Active" },
  { value: "paused", label: "Paused" },
  { value: "rejected", label: "Declined" },
];

export default async function AdminArtistsPage({
  searchParams,
}: {
  searchParams: Promise<Search>;
}) {
  const sp = await searchParams;
  const status = TABS.find((t) => t.value === sp.status)?.value ?? "pending";

  const page = await listAdminArtists({ status, cursor: sp.cursor }).catch(
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
          <h1 className="font-serif text-3xl tracking-tight">Artists</h1>
        </header>

        {/* Status tabs — link-based so the URL is the source of truth
            and the page can be bookmarked. */}
        <nav className="mb-6 flex flex-wrap gap-2" aria-label="Status filter">
          {TABS.map((t) => (
            <Link
              key={t.value}
              href={`/admin/artists?status=${t.value}`}
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
          <p className="text-sm text-muted">No artists in this queue.</p>
        ) : (
          <ul className="divide-y divide-border border border-border bg-surface">
            {items.map((a) => (
              <AdminArtistRow key={a.id} artist={a} />
            ))}
          </ul>
        )}

        {page?.next_cursor && (
          <div className="mt-6 text-center">
            <Link
              href={`/admin/artists?status=${status}&cursor=${page.next_cursor}`}
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

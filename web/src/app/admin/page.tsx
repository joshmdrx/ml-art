import type { Metadata } from "next";
import Link from "next/link";
import { TopNav } from "@/components/TopNav";
import { listAdminArtists } from "@/lib/api";

export const metadata: Metadata = {
  title: "Admin — Wander",
  robots: { index: false, follow: false },
};

/**
 * `/admin` — index landing for the admin surface.
 *
 * Surfaces queue counts so the admin sees at a glance what needs
 * attention. Sub-queues live at `/admin/artists`, `/admin/images`,
 * and `/admin/venues` (when T-081 lands).
 */
export default async function AdminIndexPage() {
  // Cheap fan-out for the queue counts. Each call is paginated at
  // 24; we only need the first page to detect non-empty, so the cost
  // is bounded.
  const [pending, paused] = await Promise.all([
    listAdminArtists({ status: "pending" }).catch(() => null),
    listAdminArtists({ status: "paused" }).catch(() => null),
  ]);

  const pendingCount = pending?.items.length ?? 0;
  const pausedCount = paused?.items.length ?? 0;

  return (
    <>
      <TopNav />
      <main className="flex-1 mx-auto w-full max-w-screen-lg px-6 py-12">
        <header className="mb-10">
          <h1 className="font-serif text-3xl tracking-tight">Admin</h1>
          <p className="mt-2 text-sm text-muted">
            Internal queues for approval workflows. Not visible to
            non-admin users.
          </p>
        </header>

        <section className="grid sm:grid-cols-2 gap-4">
          <AdminTile
            href="/admin/artists?status=pending"
            title="Artist applications"
            description="Self-signed-up artists awaiting approval."
            count={pendingCount}
            countLabel="pending"
          />
          <AdminTile
            href="/admin/artists?status=paused"
            title="Paused artists"
            description="Active accounts taken offline for review."
            count={pausedCount}
            countLabel="paused"
          />
        </section>
      </main>
    </>
  );
}

function AdminTile({
  href,
  title,
  description,
  count,
  countLabel,
}: {
  href: string;
  title: string;
  description: string;
  count: number;
  countLabel: string;
}) {
  return (
    <Link
      href={href}
      className="block border border-border bg-surface p-5 hover:bg-background transition-colors"
    >
      <div className="flex items-start justify-between gap-3">
        <div>
          <h2 className="font-serif text-xl">{title}</h2>
          <p className="mt-1 text-sm text-muted">{description}</p>
        </div>
        <div className="text-right shrink-0">
          <span className="font-serif text-2xl">{count}</span>
          <p className="text-xs text-muted">{countLabel}</p>
        </div>
      </div>
    </Link>
  );
}

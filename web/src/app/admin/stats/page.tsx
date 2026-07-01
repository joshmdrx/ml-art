import type { Metadata } from "next";
import Link from "next/link";
import { TopNav } from "@/components/TopNav";
import { getAdminStats, type WeeklyFunnel } from "@/lib/api";

export const metadata: Metadata = {
  title: "Admin · Stats — Wander",
  robots: { index: false, follow: false },
};

/**
 * T-084.1 — operator stats page. Server-rendered read-only view over
 * the platform's core counts + a 4-week events funnel + recent admin
 * activity. Every metric is queried in a single round trip via
 * `/v1/admin/stats`.
 *
 * Everything here is a canned aggregate — no filters, no drill-down.
 * When something surprising surfaces the answer is "psql the events
 * table," not "add another chart." Kept intentionally simple so it
 * scales to whatever pre-launch signal we get without becoming a
 * dashboard product itself.
 */
export default async function AdminStatsPage() {
  const stats = await getAdminStats().catch(() => null);

  if (!stats) {
    return (
      <>
        <TopNav />
        <main className="flex-1 mx-auto w-full max-w-screen-lg px-6 py-12">
          <p className="text-sm text-muted">
            Couldn&apos;t load stats. Check the API + try again.
          </p>
        </main>
      </>
    );
  }

  const { counts, weekly_funnel, admin_activity } = stats;

  return (
    <>
      <TopNav />
      <main className="flex-1 mx-auto w-full max-w-screen-lg px-6 py-12">
        <header className="mb-8">
          <p className="text-xs text-muted mb-1">
            <Link href="/admin" className="hover:text-foreground">
              ← Admin
            </Link>
          </p>
          <h1 className="font-serif text-3xl tracking-tight">Stats</h1>
          <p className="mt-2 text-sm text-muted">
            Snapshot of platform activity. Refreshes on page load.
          </p>
        </header>

        {/* Big-number tiles */}
        <section className="grid grid-cols-2 md:grid-cols-4 gap-4 mb-10">
          <CountTile
            label="Users"
            total={counts.users.total}
            last7d={counts.users.last_7d}
            last30d={counts.users.last_30d}
          />
          <CountTile
            label="Active artists"
            total={counts.artists_active.total}
            last7d={counts.artists_active.last_7d}
            last30d={counts.artists_active.last_30d}
          />
          <CountTile
            label="Published works"
            total={counts.artworks_published.total}
            last7d={counts.artworks_published.last_7d}
            last30d={counts.artworks_published.last_30d}
          />
          <CountTile
            label="Delivered inquiries"
            total={counts.inquiries_delivered.total}
            last7d={counts.inquiries_delivered.last_7d}
            last30d={counts.inquiries_delivered.last_30d}
          />
        </section>

        {/* 4-week funnel */}
        <section className="mb-10">
          <h2 className="font-serif text-xl mb-4">Search → inquiry funnel</h2>
          <FunnelTable weeks={weekly_funnel} />
          <p className="mt-2 text-xs text-muted">
            Each week runs Monday–Sunday, aggregated from the{" "}
            <code className="font-mono">events</code> table.
          </p>
        </section>

        {/* Admin activity */}
        <section>
          <h2 className="font-serif text-xl mb-4">Admin activity</h2>
          <p className="text-sm">
            <strong>{admin_activity.mutations_last_7d}</strong> admin{" "}
            {admin_activity.mutations_last_7d === 1
              ? "mutation"
              : "mutations"}{" "}
            in the last 7 days.
            {admin_activity.last_mutation_at && (
              <>
                {" "}
                Last one:{" "}
                <time dateTime={admin_activity.last_mutation_at}>
                  {new Date(admin_activity.last_mutation_at).toLocaleString()}
                </time>
                .
              </>
            )}
          </p>
          <p className="mt-2 text-xs text-muted">
            <Link
              href="/admin/audit-log"
              className="underline underline-offset-2"
            >
              Open audit log →
            </Link>
          </p>
        </section>
      </main>
    </>
  );
}

function CountTile({
  label,
  total,
  last7d,
  last30d,
}: {
  label: string;
  total: number;
  last7d: number;
  last30d: number;
}) {
  return (
    <div className="border border-border bg-surface p-4">
      <p className="text-xs uppercase tracking-wide text-muted mb-2">
        {label}
      </p>
      <p className="font-serif text-3xl">{total.toLocaleString()}</p>
      <p className="mt-2 text-xs text-muted">
        +{last7d.toLocaleString()} · 7d
        <br />
        +{last30d.toLocaleString()} · 30d
      </p>
    </div>
  );
}

function FunnelTable({ weeks }: { weeks: WeeklyFunnel[] }) {
  return (
    <div className="border border-border bg-surface overflow-x-auto">
      <table className="w-full text-sm">
        <thead>
          <tr className="border-b border-border">
            <Th>Week of</Th>
            <Th align="right">Searches</Th>
            <Th align="right">Artwork views</Th>
            <Th align="right">Inquiry starts</Th>
            <Th align="right">Inquiries sent</Th>
          </tr>
        </thead>
        <tbody>
          {weeks.map((w) => (
            <tr key={w.week} className="border-b border-border last:border-0">
              <Td>
                <time dateTime={w.week}>
                  {new Date(w.week).toLocaleDateString(undefined, {
                    month: "short",
                    day: "numeric",
                  })}
                </time>
              </Td>
              <Td align="right">{w.searches.toLocaleString()}</Td>
              <Td align="right">{w.views.toLocaleString()}</Td>
              <Td align="right">{w.inquiries_started.toLocaleString()}</Td>
              <Td align="right">{w.inquiries_submitted.toLocaleString()}</Td>
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}

function Th({
  children,
  align = "left",
}: {
  children: React.ReactNode;
  align?: "left" | "right";
}) {
  return (
    <th
      className={`px-3 py-2 text-xs uppercase tracking-wide text-muted font-normal ${
        align === "right" ? "text-right" : "text-left"
      }`}
    >
      {children}
    </th>
  );
}

function Td({
  children,
  align = "left",
}: {
  children: React.ReactNode;
  align?: "left" | "right";
}) {
  return (
    <td
      className={`px-3 py-2 tabular-nums ${
        align === "right" ? "text-right" : "text-left"
      }`}
    >
      {children}
    </td>
  );
}

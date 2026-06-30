import type { Metadata } from "next";
import Link from "next/link";
import { TopNav } from "@/components/TopNav";
import { listAdminAuditLog } from "@/lib/api";

export const metadata: Metadata = {
  title: "Admin · Audit log — Wander",
  robots: { index: false, follow: false },
};

type Search = { cursor?: string };

/**
 * T-083.5 — read-only audit log viewer.
 *
 * Reverse-chronological feed of every admin mutation. No filtering in
 * v1 — the table will be tiny for years and a flat scroll reads
 * easier than a faceted browser for this size of data set.
 */
export default async function AdminAuditLogPage({
  searchParams,
}: {
  searchParams: Promise<Search>;
}) {
  const sp = await searchParams;
  const page = await listAdminAuditLog({ cursor: sp.cursor }).catch(() => null);
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
          <h1 className="font-serif text-3xl tracking-tight">Audit log</h1>
          <p className="mt-2 text-sm text-muted">
            Every admin mutation, newest first. System actions
            (auto-promotion, scheduled jobs) show no admin.
          </p>
        </header>

        {items.length === 0 ? (
          <p className="text-sm text-muted">No audit entries yet.</p>
        ) : (
          <ul className="divide-y divide-border border border-border bg-surface">
            {items.map((it) => (
              <li key={it.id} className="p-4">
                <div className="flex items-baseline justify-between gap-4 flex-wrap">
                  <div className="min-w-0">
                    <span className="font-mono text-sm">{it.action}</span>{" "}
                    <span className="text-xs text-muted">
                      ({it.target_kind})
                    </span>
                  </div>
                  <div className="text-xs text-muted shrink-0">
                    <time dateTime={it.created_at}>
                      {new Date(it.created_at).toLocaleString()}
                    </time>
                  </div>
                </div>
                <p className="mt-1 text-xs text-muted">
                  {it.admin_email ?? <em>system</em>}
                  {it.target_id && (
                    <>
                      {" · "}target{" "}
                      <span className="font-mono">{it.target_id}</span>
                    </>
                  )}
                </p>
                {/* Inline before/after preview — kept compact; full
                    JSON is available via the API if needed. */}
                {(it.before_jsonb != null || it.after_jsonb != null) && (
                  <details className="mt-2">
                    <summary className="text-xs text-muted cursor-pointer hover:text-foreground">
                      View diff
                    </summary>
                    <pre className="mt-2 text-xs bg-background p-2 overflow-x-auto">
                      {JSON.stringify(
                        { before: it.before_jsonb, after: it.after_jsonb },
                        null,
                        2,
                      )}
                    </pre>
                  </details>
                )}
              </li>
            ))}
          </ul>
        )}

        {page?.next_cursor && (
          <div className="mt-6 text-center">
            <Link
              href={`/admin/audit-log?cursor=${page.next_cursor}`}
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

import type { Metadata } from "next";
import Link from "next/link";
import { TopNav } from "@/components/TopNav";
import { DraftCreateForm } from "@/components/admin/DraftCreateForm";
import { DraftRow } from "@/components/admin/DraftRow";
import { listAdminDrafts } from "@/lib/api";
import { reportError } from "@/lib/reportError";

/**
 * T-086 — `/admin/drafts`: consent-first example pages for galleries
 * and studios. Create a draft once they've said yes, build it in the
 * studio, send a private link, and they claim or remove it. Admin gate
 * is the `/admin` layout.
 */

export const metadata: Metadata = {
  title: "Admin · Drafts",
  robots: { index: false, follow: false },
};

export default async function AdminDraftsPage() {
  const page = await listAdminDrafts().catch((e) => {
    reportError(e, { surface: "admin-drafts" });
    return null;
  });
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
          <h1 className="font-serif text-3xl tracking-tight">Draft pages</h1>
          <p className="mt-2 text-sm text-muted max-w-2xl">
            Example pages for organisations that asked for one. Drafts
            never appear on public pages. They become the
            organisation&apos;s when they claim the private link, and
            go live only when they publish.
          </p>
        </header>

        <DraftCreateForm />

        <section className="mt-10">
          {page === null ? (
            <p className="text-sm text-muted">Couldn&apos;t load drafts.</p>
          ) : items.length === 0 ? (
            <p className="text-sm text-muted">No drafts yet.</p>
          ) : (
            <ul className="divide-y divide-border border border-border bg-surface">
              {items.map((d) => (
                <DraftRow key={d.id} draft={d} />
              ))}
            </ul>
          )}
        </section>
      </main>
    </>
  );
}

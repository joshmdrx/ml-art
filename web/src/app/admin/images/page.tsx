import type { Metadata } from "next";
import Link from "next/link";
import { TopNav } from "@/components/TopNav";
import { AdminImageRow } from "@/components/admin/AdminImageRow";
import { listAdminImages } from "@/lib/api";

export const metadata: Metadata = {
  title: "Admin · Images — Wander",
  robots: { index: false, follow: false },
};

type Search = { cursor?: string };

/**
 * T-083.3 — auto-moderated-out image queue.
 *
 * Lists `artwork_images` rows with `moderation_status='rejected'` so
 * the admin can review false-positive rejections from the auto-mod
 * pipeline (T-008) and override them back to `approved`.
 *
 * Status filter is implicit (rejected is the only queue that matters);
 * `pending` and `approved` aren't exposed here. If we ever need a
 * "approved that look wrong" view we'll add a tab strip like
 * /admin/artists has.
 */
export default async function AdminImagesPage({
  searchParams,
}: {
  searchParams: Promise<Search>;
}) {
  const sp = await searchParams;
  const page = await listAdminImages({ status: "rejected", cursor: sp.cursor }).catch(
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
          <h1 className="font-serif text-3xl tracking-tight">Image moderation</h1>
          <p className="mt-2 text-sm text-muted">
            Auto-moderator (T-008) flagged these images. Override if the
            rejection was wrong; otherwise leave them — the image stays
            hidden on public surfaces until approved.
          </p>
        </header>

        {items.length === 0 ? (
          <p className="text-sm text-muted">
            No rejected images. Auto-moderation is keeping up.
          </p>
        ) : (
          <ul className="grid sm:grid-cols-2 gap-4">
            {items.map((it) => (
              <AdminImageRow key={it.id} image={it} />
            ))}
          </ul>
        )}

        {page?.next_cursor && (
          <div className="mt-6 text-center">
            <Link
              href={`/admin/images?cursor=${page.next_cursor}`}
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

import type { Metadata } from "next";
import Link from "next/link";
import { redirect } from "next/navigation";
import clsx from "clsx";
import { auth } from "@clerk/nextjs/server";
import { InquiryInbox } from "@/components/studio/InquiryInbox";
import { getStudioMe, listStudioInquiries } from "@/lib/api";
import { reportError } from "@/lib/reportError";

export const metadata: Metadata = {
  title: "Studio inquiries",
};

type Search = { status?: string; id?: string };
type StatusFilter = "all" | "pending" | "delivered";

const FILTERS: { token: StatusFilter; label: string }[] = [
  { token: "all", label: "All" },
  { token: "pending", label: "Pending" },
  { token: "delivered", label: "Delivered" },
];

/**
 * `/studio/inquiries` — T-011 Phase 4 + 4b.
 *
 * Server-renders the filter chrome + initial list; the actual
 * inquiry rows + reply forms live in `<InquiryInbox>` (client).
 * That split lets us:
 *   - keep `auth()` + the API fetch on the server (no token
 *     plumbing on the client),
 *   - manage per-card reply form state without a re-render
 *     storm on every keystroke,
 *   - auto-fire mark-as-read on view without a server roundtrip
 *     re-rendering the page.
 */
export default async function StudioInquiriesPage({
  searchParams,
}: {
  searchParams: Promise<Search>;
}) {
  const { userId } = await auth();
  if (!userId) {
    redirect(
      "/sign-in?redirect_url=" + encodeURIComponent("/studio/inquiries")
    );
  }

  const sp = await searchParams;
  const status: StatusFilter =
    sp.status === "pending" || sp.status === "delivered" ? sp.status : "all";

  const [artist, page] = await Promise.all([
    getStudioMe().catch((e) => {
      reportError(e, { surface: "studio-inquiries", call: "me" });
      return null;
    }),
    listStudioInquiries({ status }).catch((e) => {
      reportError(e, { surface: "studio-inquiries", call: "list" });
      return null;
    }),
  ]);

  if (!artist) {
    redirect("/onboarding");
  }

  const items = page?.items ?? [];

  return (
    <>
      <header className="mb-8">
        <h1 className="font-serif text-3xl tracking-tight">Inquiries</h1>
        <p className="mt-2 text-sm text-muted">
          {artist.display_name} — messages about your work
        </p>
      </header>

      <div className="flex items-center justify-between mb-6">
        <div
          role="toolbar"
          aria-label="Filter by status"
          className="flex gap-2"
        >
          {FILTERS.map((f) => {
            const href =
              f.token === "all"
                ? "/studio/inquiries"
                : `/studio/inquiries?status=${f.token}`;
            const active = status === f.token;
            return (
              <Link
                key={f.token}
                href={href}
                aria-current={active ? "page" : undefined}
                className={clsx(
                  "px-3 py-1.5 text-sm border",
                  active
                    ? "border-foreground bg-foreground text-background"
                    : "border-border bg-surface hover:bg-background"
                )}
              >
                {f.label}
              </Link>
            );
          })}
        </div>
      </div>

      {items.length === 0 ? (
        <EmptyState status={status} />
      ) : (
        <InquiryInbox initialItems={items} selectedId={sp.id ?? null} />
      )}
    </>
  );
}

function EmptyState({ status }: { status: StatusFilter }) {
  if (status !== "all") {
    return (
      <div className="text-center py-16 text-sm text-muted">
        No {status === "pending" ? "pending" : "delivered"} inquiries.{" "}
        <Link
          href="/studio/inquiries"
          className="underline underline-offset-2 hover:text-foreground"
        >
          Show all
        </Link>
        .
      </div>
    );
  }
  return (
    <div className="text-center py-16 text-sm text-muted">
      No inquiries yet. When a collector reaches out about one of your works,
      it&apos;ll show up here.
    </div>
  );
}

import type { Metadata } from "next";
import Link from "next/link";
import { redirect } from "next/navigation";
import clsx from "clsx";
import { auth } from "@clerk/nextjs/server";
import { TopNav } from "@/components/TopNav";
import { getStudioMe, listStudioInquiries, type StudioInquiry } from "@/lib/api";
import { reportError } from "@/lib/reportError";

export const metadata: Metadata = {
  title: "Inquiries — Studio",
};

type Search = { status?: string };
type StatusFilter = "all" | "pending" | "delivered";

const FILTERS: { token: StatusFilter; label: string }[] = [
  { token: "all", label: "All" },
  { token: "pending", label: "Pending" },
  { token: "delivered", label: "Delivered" },
];

/**
 * `/studio/inquiries` — T-011 Phase 4.
 *
 * Read-only list of inquiries addressed to the calling artist. The
 * artist already gets the notification email (T-032); this page is the
 * in-app companion so they can re-read past messages, see pending
 * anonymous inquiries (waiting on the verification-link click), and
 * filter by status.
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
      <TopNav />
      <main className="flex-1 mx-auto w-full max-w-screen-2xl px-6 py-12">
        <header className="flex items-baseline justify-between mb-8">
          <div>
            <h1 className="font-serif text-3xl tracking-tight">Inquiries</h1>
            <p className="mt-2 text-sm text-muted">
              {artist.display_name} — messages about your work
            </p>
          </div>
          <Link
            href="/studio"
            className="text-sm underline underline-offset-2 text-muted hover:text-foreground"
          >
            ← Back to studio
          </Link>
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
          <ul className="flex flex-col gap-3">
            {items.map((inq) => (
              <li key={inq.id}>
                <InquiryCard inquiry={inq} />
              </li>
            ))}
          </ul>
        )}
      </main>
    </>
  );
}

function InquiryCard({ inquiry }: { inquiry: StudioInquiry }) {
  const created = new Date(inquiry.created_at);
  return (
    <article className="border border-border bg-surface p-4 flex gap-4">
      {inquiry.artwork_primary_image_url ? (
        // eslint-disable-next-line @next/next/no-img-element
        <img
          src={inquiry.artwork_primary_image_url}
          alt=""
          className="w-20 h-20 object-cover bg-background flex-shrink-0"
        />
      ) : (
        <div className="w-20 h-20 bg-background flex-shrink-0" aria-hidden />
      )}
      <div className="min-w-0 flex-1">
        <div className="flex items-baseline justify-between gap-3">
          <div className="min-w-0">
            <p className="text-sm">
              <span className="font-medium">{inquiry.from_name}</span>{" "}
              <a
                href={`mailto:${inquiry.from_email}`}
                className="text-muted hover:underline"
              >
                &lt;{inquiry.from_email}&gt;
              </a>
            </p>
            <p className="text-xs text-muted mt-0.5">
              About{" "}
              <Link
                href={`/artworks/${inquiry.artwork_id}`}
                className="underline underline-offset-2 hover:text-foreground"
              >
                {inquiry.artwork_title ?? "an artwork"}
              </Link>{" "}
              · {formatRelative(created)}
              {inquiry.budget_range ? (
                <>
                  {" · "}
                  Budget: <span className="text-foreground">{inquiry.budget_range}</span>
                </>
              ) : null}
            </p>
          </div>
          <StatusBadge status={inquiry.status} />
        </div>
        <p className="mt-3 text-sm whitespace-pre-line">{inquiry.message}</p>
      </div>
    </article>
  );
}

function StatusBadge({ status }: { status: StudioInquiry["status"] }) {
  if (status === "delivered") {
    return (
      <span className="text-xs px-2 py-0.5 border border-border text-muted">
        Delivered
      </span>
    );
  }
  return (
    <span className="text-xs px-2 py-0.5 border border-border bg-background">
      Awaiting verification
    </span>
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

/**
 * Tiny "5m ago / 2h ago / 3d ago / Jan 4" formatter. Anything older
 * than a week falls back to a short date. Kept inline because no other
 * page uses it yet; promote to `lib/` when the second caller lands.
 */
function formatRelative(d: Date): string {
  const now = Date.now();
  const diff = Math.max(0, now - d.getTime());
  const minutes = Math.floor(diff / 60_000);
  if (minutes < 1) return "just now";
  if (minutes < 60) return `${minutes}m ago`;
  const hours = Math.floor(minutes / 60);
  if (hours < 24) return `${hours}h ago`;
  const days = Math.floor(hours / 24);
  if (days < 7) return `${days}d ago`;
  return d.toLocaleDateString(undefined, { month: "short", day: "numeric" });
}

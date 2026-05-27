import type { Metadata } from "next";
import Link from "next/link";
import { redirect } from "next/navigation";
import { auth } from "@clerk/nextjs/server";
import { TopNav } from "@/components/TopNav";
import { StudioPortfolio } from "@/components/StudioPortfolio";
import { getStudioMe, listMyArtworks } from "@/lib/api";
import { reportError } from "@/lib/reportError";

export const metadata: Metadata = {
  title: "Studio — ml-art",
};

type Search = {
  status?: string;
};

/**
 * `/studio` — the artist's portfolio dashboard.
 *
 *   - Signed-out → /sign-in?redirect_url=/studio
 *   - Signed-in non-artist → empty state with link to /studio/settings
 *     (where the same empty state explains)
 *   - Signed-in artist → grid of their artworks with status filter +
 *     "New artwork" affordance
 */
export default async function StudioPage({
  searchParams,
}: {
  searchParams: Promise<Search>;
}) {
  const { userId } = await auth();
  if (!userId) {
    redirect("/sign-in?redirect_url=" + encodeURIComponent("/studio"));
  }
  const sp = await searchParams;
  const statusParam = sp.status;
  const status =
    statusParam === "draft" ||
    statusParam === "published" ||
    statusParam === "archived"
      ? statusParam
      : "all";

  // Run me + list in parallel; both calls return null for non-artists
  // so the page can fall through to the empty state without throwing.
  const [artist, page] = await Promise.all([
    getStudioMe().catch((e) => {
      reportError(e, { surface: "studio-portfolio", call: "me" });
      return null;
    }),
    listMyArtworks({ status: status === "all" ? undefined : status }).catch(
      (e) => {
        reportError(e, { surface: "studio-portfolio", call: "list" });
        return null;
      }
    ),
  ]);

  return (
    <>
      <TopNav />
      <main className="flex-1 mx-auto w-full max-w-screen-2xl px-6 py-12">
        <header className="flex items-baseline justify-between mb-8">
          <div>
            <h1 className="font-serif text-3xl tracking-tight">Studio</h1>
            <p className="mt-2 text-sm text-muted">
              {artist
                ? `${artist.display_name} — ${artist.status === "active" ? "Published" : "Unpublished"}`
                : "Your portfolio"}
            </p>
          </div>
          <Link
            href="/studio/settings"
            className="text-sm underline underline-offset-2 text-muted hover:text-foreground"
          >
            Settings →
          </Link>
        </header>

        {artist && page ? (
          <StudioPortfolio
            artist={artist}
            items={page.items}
            status={status}
          />
        ) : (
          <NotAnArtistYet />
        )}
      </main>
    </>
  );
}

function NotAnArtistYet() {
  return (
    <section className="p-6 border border-border bg-surface">
      <h2 className="font-serif text-xl">No portfolio yet.</h2>
      <p className="mt-3 text-sm leading-relaxed">
        Studio is for verified artists on the platform. We&apos;re onboarding
        artists by direct invitation only right now —{" "}
        <Link href="/" className="underline">
          head back to the homepage
        </Link>{" "}
        or get in touch.
      </p>
    </section>
  );
}

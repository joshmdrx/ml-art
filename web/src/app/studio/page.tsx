import type { Metadata } from "next";
import { redirect } from "next/navigation";
import { auth } from "@clerk/nextjs/server";
import { StudioPortfolio } from "@/components/StudioPortfolio";
import { getStudioMe, listMyArtworks } from "@/lib/api";
import { reportError } from "@/lib/reportError";

export const metadata: Metadata = {
  title: "Studio",
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

  // Run me + list in parallel. `getStudioMe` returns null for signed-
  // in non-artists (404 → null); send those users to the onboarding
  // wizard so they can mint an artist row instead of bouncing off an
  // empty state. With T-012 Phase 1 shipped, self-onboarding is the
  // default path.
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

  if (!artist) {
    redirect("/onboarding");
  }

  // `page === null` means the list endpoint errored (not that the
  // artist has zero rows — the API returns an empty `items` array in
  // that case). Fall back to an empty grid; the StudioPortfolio
  // component handles `items: []` cleanly.
  const items = page?.items ?? [];

  return (
    <>
      <header className="mb-8">
        <h1 className="font-serif text-3xl tracking-tight">Portfolio</h1>
        <p className="mt-2 text-sm text-muted">
          {artist.display_name} —{" "}
          {artist.status === "active" ? "Published" : "Unpublished"}
        </p>
        {/* T-052 — follower count surfaced on the dashboard.
            Defaults to 0 if the API hasn't been redeployed yet. */}
        {(artist.follower_count ?? 0) > 0 && (
          <p className="mt-1 text-xs text-muted">
            {artist.follower_count}{" "}
            {artist.follower_count === 1 ? "follower" : "followers"}
          </p>
        )}
      </header>

      <StudioPortfolio artist={artist} items={items} status={status} />
    </>
  );
}

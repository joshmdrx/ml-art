import type { Metadata } from "next";
import { redirect } from "next/navigation";
import { auth } from "@clerk/nextjs/server";
import { StudioSeriesManager } from "@/components/StudioSeriesManager";
import {
  getStudioMe,
  listStudioSeries,
  listMyArtworks,
} from "@/lib/api";
import { reportError } from "@/lib/reportError";

export const metadata: Metadata = {
  title: "Series — Studio",
};

/**
 * `/studio/series` — T-058.2.
 *
 * Authenticated-artist page that lists their curated series and lets
 * them create, edit, and manage membership via a checkbox grid.
 *
 * Reads three things on the server: the studio artist row (gates
 * access), the series list, and the artist's *full* artworks list
 * (drafts + published) — the latter feeds the membership grid in
 * the edit modal without needing a second round-trip on open.
 */
export default async function StudioSeriesPage() {
  const { userId } = await auth();
  if (!userId) {
    redirect("/sign-in?redirect_url=" + encodeURIComponent("/studio/series"));
  }

  const [artist, series, artworks] = await Promise.all([
    getStudioMe().catch((e) => {
      reportError(e, { surface: "studio-series", call: "me" });
      return null;
    }),
    listStudioSeries().catch((e) => {
      reportError(e, { surface: "studio-series", call: "list" });
      return null;
    }),
    listMyArtworks().catch((e) => {
      reportError(e, { surface: "studio-series", call: "list-artworks" });
      return null;
    }),
  ]);

  if (!artist) {
    redirect("/onboarding");
  }

  const items = series?.items ?? [];
  const allArtworks = artworks?.items ?? [];

  return (
    <>
      <header className="mb-8">
        <h1 className="font-serif text-3xl tracking-tight">Series</h1>
        <p className="mt-2 text-sm text-muted">
          Group your work into projects or themes. Each series gets its
          own page on your artist profile.
        </p>
      </header>

      <StudioSeriesManager
        artist={artist}
        initialSeries={items}
        artworks={allArtworks}
      />
    </>
  );
}

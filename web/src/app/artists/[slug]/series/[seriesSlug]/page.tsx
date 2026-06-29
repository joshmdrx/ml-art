import Link from "next/link";
import { notFound } from "next/navigation";
import type { Metadata } from "next";
import { TopNav } from "@/components/TopNav";
import { ArtworkGrid } from "@/components/ArtworkGrid";
import { getArtistSeriesDetail } from "@/lib/api";
import { reportError } from "@/lib/reportError";

/**
 * `/artists/[slug]/series/[seriesSlug]` — public series page (T-058.3).
 *
 * Shareable URL for a single series — header with cover + title +
 * statement, then the grid of member artworks. The artist's name
 * links back to the main artist page.
 *
 * 404s when the series is empty or doesn't exist (matches the api).
 */

type Params = Promise<{ slug: string; seriesSlug: string }>;

export async function generateMetadata({
  params,
}: {
  params: Params;
}): Promise<Metadata> {
  const { slug, seriesSlug } = await params;
  const data = await getArtistSeriesDetail(slug, seriesSlug).catch(() => null);
  if (!data) return { title: "Series not found" };
  return {
    title: `${data.series.title} — ${data.artist.display_name}`,
    description: data.series.statement?.slice(0, 160) ?? undefined,
  };
}

export default async function SeriesPage({ params }: { params: Params }) {
  const { slug, seriesSlug } = await params;

  const data = await getArtistSeriesDetail(slug, seriesSlug).catch((e) => {
    reportError(e, { surface: "series-detail", slug, seriesSlug });
    return null;
  });
  if (!data) notFound();

  const { series, artist, artworks } = data;

  return (
    <>
      <TopNav />

      <main className="flex-1 mx-auto w-full max-w-screen-2xl px-6 py-12 md:py-16">
        {/* Crumb back to the artist */}
        <p className="text-sm text-muted mb-6">
          <Link
            href={`/artists/${artist.slug}?view=series`}
            className="underline hover:text-foreground"
          >
            ← {artist.display_name}
          </Link>
        </p>

        <header className="grid grid-cols-1 md:grid-cols-[1fr_2fr] gap-8 mb-12 md:mb-16">
          <div className="aspect-square bg-background border border-border overflow-hidden">
            {series.cover_image_url ? (
              // eslint-disable-next-line @next/next/no-img-element
              <img
                src={series.cover_image_url}
                alt={series.title}
                className="w-full h-full object-cover"
              />
            ) : (
              <div className="w-full h-full flex items-center justify-center text-muted text-xs">
                No cover image
              </div>
            )}
          </div>
          <div>
            <h1 className="font-serif text-4xl md:text-5xl tracking-tight">
              {series.title}
            </h1>
            <p className="mt-2 text-sm text-muted">
              by{" "}
              <Link
                href={`/artists/${artist.slug}`}
                className="underline hover:text-foreground"
              >
                {artist.display_name}
              </Link>
              {" · "}
              {series.artwork_count}{" "}
              {series.artwork_count === 1 ? "work" : "works"}
            </p>
            {series.statement && (
              <p className="mt-6 text-base leading-relaxed whitespace-pre-line max-w-prose">
                {series.statement}
              </p>
            )}
          </div>
        </header>

        <section>
          <ArtworkGrid items={artworks.items} />
        </section>
      </main>
    </>
  );
}

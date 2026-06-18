import { notFound } from "next/navigation";
import type { Metadata } from "next";
import { TopNav } from "@/components/TopNav";
import { ArtworkGrid } from "@/components/ArtworkGrid";
import { ArtistLocationsMap } from "@/components/ArtistLocationsMap";
import { BackToSearchLink } from "@/components/BackToSearchLink";
import { getArtist } from "@/lib/api";
import { reportError } from "@/lib/reportError";

/**
 * /artists/[slug] — artist portfolio.
 *
 * Header: name (large, serif), location (small, muted), bio (2–3 lines),
 *         links row (website + socials).
 * Statement: optional, expandable later.
 * Works:    grid of all published artworks.
 *
 * Sections deferred until those APIs land:
 *   - "Similar artists" row (needs /v1/artists/:slug/similar)
 *   - All / Available tabs (needs filter UI + paginated artworks endpoint)
 */

type Params = Promise<{ slug: string }>;

export async function generateMetadata({
  params,
}: {
  params: Params;
}): Promise<Metadata> {
  const { slug } = await params;
  const data = await getArtist(slug).catch(() => null);
  if (!data) return { title: "Artist not found" };
  const a = data.artist;
  return {
    title: a.display_name,
    description: a.bio?.slice(0, 160) ?? `Works by ${a.display_name}`,
  };
}

export default async function ArtistPage({ params }: { params: Params }) {
  const { slug } = await params;

  const data = await getArtist(slug).catch((e) => {
    reportError(e, { surface: "artist-detail", slug });
    return null;
  });
  if (!data) notFound();

  // `locations` defaults to [] so a stale API build (no `locations`
  // field in the JSON) doesn't crash the page — the map widget
  // tolerates the empty list by rendering nothing.
  const { artist, artworks } = data;
  const locations = data.locations ?? [];

  // Order socials deterministically for display. Empty object → []
  const socials = Object.entries(artist.socials ?? {}).filter(
    ([, v]) => typeof v === "string" && v.length > 0
  );

  return (
    <>
      <TopNav />

      <main className="flex-1 mx-auto w-full max-w-screen-2xl px-6 py-12 md:py-16">
        {/* Header */}
        <header className="max-w-3xl mb-12 md:mb-16">
          <h1 className="font-serif text-4xl md:text-5xl tracking-tight">
            {artist.display_name}
          </h1>

          {(artist.location || artist.city) && (
            <p className="mt-2 text-sm text-muted">
              {artist.location ??
                [artist.city, artist.country].filter(Boolean).join(", ")}
            </p>
          )}

          {artist.bio && (
            <p className="mt-6 text-base leading-relaxed max-w-2xl">
              {artist.bio}
            </p>
          )}

          {(artist.website_url || socials.length > 0) && (
            <ul className="mt-6 flex flex-wrap gap-x-6 gap-y-2 text-sm">
              {artist.website_url && (
                <li>
                  <a
                    href={artist.website_url}
                    target="_blank"
                    rel="noopener noreferrer"
                    className="underline hover:no-underline"
                  >
                    Website ↗
                  </a>
                </li>
              )}
              {socials.map(([k, v]) => (
                <li key={k}>
                  <a
                    href={String(v)}
                    target="_blank"
                    rel="noopener noreferrer"
                    className="underline hover:no-underline capitalize"
                  >
                    {k} ↗
                  </a>
                </li>
              ))}
            </ul>
          )}
        </header>

        {/* Where to see this work — Mapbox GL embed (T-038 G4). The
            component returns null when there are no locations, so the
            section disappears entirely for artists without any. */}
        <ArtistLocationsMap locations={locations} artistSlug={artist.slug} />

        {/* Statement (optional) */}
        {artist.artist_statement && (
          <section className="max-w-2xl mb-12 md:mb-16">
            <h2 className="font-serif text-xl mb-3">Artist statement</h2>
            <p className="text-base leading-relaxed whitespace-pre-line">
              {artist.artist_statement}
            </p>
          </section>
        )}

        {/* Works */}
        <section>
          <div className="flex items-baseline justify-between mb-6">
            <h2 className="font-serif text-xl">Works</h2>
            {/* Filter tabs (All / Available) deferred until availability is real */}
          </div>
          {artworks.items.length === 0 ? (
            <p className="text-muted text-sm">
              This artist hasn’t published any work yet.
            </p>
          ) : (
            <ArtworkGrid items={artworks.items} />
          )}
        </section>

        <p className="mt-16 text-xs text-muted">
          <BackToSearchLink className="hover:text-foreground" />
        </p>
      </main>
    </>
  );
}

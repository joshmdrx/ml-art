import Link from "next/link";
import { notFound } from "next/navigation";
import type { Metadata } from "next";
import { clsx } from "clsx";
import { TopNav } from "@/components/TopNav";
import { ArtworkGrid } from "@/components/ArtworkGrid";
import { ArtistLocationsMap } from "@/components/ArtistLocationsMap";
import { BackToSearchLink } from "@/components/BackToSearchLink";
import { FollowButton } from "@/components/FollowButton";
import { AdminArtistBanner } from "@/components/admin/AdminArtistBanner";
import { getArtist, getMe, listArtistSeries, type PublicSeries } from "@/lib/api";
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
type Search = Promise<{ view?: string }>;

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

export default async function ArtistPage({
  params,
  searchParams,
}: {
  params: Params;
  searchParams: Search;
}) {
  const { slug } = await params;
  const sp = await searchParams;
  const view: "works" | "series" = sp.view === "series" ? "series" : "works";

  // T-058.3 — fetch series alongside the artist so we know whether to
  // show the Works/Series toggle even on the default "works" view.
  // Empty list → toggle is hidden entirely.
  // T-083 — also fetch `me` in parallel; admins see a preview banner
  // when the artist isn't publicly active. Non-admins get `null` from
  // getMe() and the banner never renders.
  const [data, seriesList, me] = await Promise.all([
    getArtist(slug).catch((e) => {
      reportError(e, { surface: "artist-detail", slug });
      return null;
    }),
    listArtistSeries(slug).catch((e) => {
      reportError(e, { surface: "artist-detail-series", slug });
      return { items: [] };
    }),
    getMe().catch(() => null),
  ]);
  if (!data) notFound();

  // `locations` defaults to [] so a stale API build (no `locations`
  // field in the JSON) doesn't crash the page — the map widget
  // tolerates the empty list by rendering nothing.
  const { artist, artworks } = data;
  const locations = data.locations ?? [];
  const series = seriesList.items;
  const hasSeries = series.length > 0;

  // Order socials deterministically for display. Empty object → []
  const socials = Object.entries(artist.socials ?? {}).filter(
    ([, v]) => typeof v === "string" && v.length > 0
  );

  const isAdminPreview = me?.is_admin && artist.status !== "active";

  return (
    <>
      <TopNav />

      <main className="flex-1 mx-auto w-full max-w-screen-2xl px-6 py-12 md:py-16">
        {/* T-083 — admin preview banner. Only renders for admins
            viewing an artist that isn't publicly `active`. Puts
            approve/decline shortcuts inline so the workflow is:
            queue → click through → decide from the artist page. */}
        {isAdminPreview && (
          <AdminArtistBanner
            artistId={artist.id}
            artistName={artist.display_name}
            status={artist.status}
          />
        )}
        {/* Header */}
        <header className="max-w-3xl mb-12 md:mb-16">
          <div className="flex items-start justify-between gap-6">
            <div>
              {/* T-085 — Gallery / space entity-type chip. Small,
                  above the headline so it reads as a category rather
                  than a label. Individual artists get no chip
                  (default state — nothing to shout about). */}
              {artist.entity_type === "gallery" && (
                <p className="mb-2 inline-block text-xs uppercase tracking-wide text-muted border border-border px-2 py-0.5">
                  Gallery
                </p>
              )}
              <h1 className="font-serif text-4xl md:text-5xl tracking-tight">
                {artist.display_name}
              </h1>

              {(artist.location || artist.city) && (
                <p className="mt-2 text-sm text-muted">
                  {artist.location ??
                    [artist.city, artist.country].filter(Boolean).join(", ")}
                </p>
              )}
            </div>

            {/* T-052 — Follow / Following. Sized to sit beside the name on
                desktop, drops below on mobile. */}
            <div className="shrink-0 flex flex-col items-end gap-1">
              <FollowButton
                artistId={artist.id}
                artistSlug={artist.slug}
                initialIsFollowing={data.is_following ?? false}
              />
              {(data.follower_count ?? 0) > 0 && (
                <p className="text-xs text-muted">
                  {data.follower_count}{" "}
                  {data.follower_count === 1 ? "follower" : "followers"}
                </p>
              )}
            </div>
          </div>

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

        {/* Works / Series — view toggle when the artist has at least
            one published series; otherwise the Works grid renders alone. */}
        <section>
          <div className="flex items-baseline justify-between mb-6">
            <h2 className="font-serif text-xl">
              {view === "series" ? "Series" : "Works"}
            </h2>
            {hasSeries && (
              <ViewToggle currentView={view} artistSlug={slug} />
            )}
          </div>
          {view === "series" ? (
            <SeriesGrid series={series} artistSlug={slug} />
          ) : artworks.items.length === 0 ? (
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

// ─────────────────────────────────────────────────────────────────────────────
// T-058.3 — series view helpers (server components)
// ─────────────────────────────────────────────────────────────────────────────

function ViewToggle({
  currentView,
  artistSlug,
}: {
  currentView: "works" | "series";
  artistSlug: string;
}) {
  return (
    <div className="flex gap-2 text-sm">
      <Link
        href={`/artists/${artistSlug}`}
        className={clsx(
          "px-3 py-1.5 border",
          currentView === "works"
            ? "border-foreground bg-foreground text-background"
            : "border-border bg-surface hover:bg-background",
        )}
      >
        Works
      </Link>
      <Link
        href={`/artists/${artistSlug}?view=series`}
        className={clsx(
          "px-3 py-1.5 border",
          currentView === "series"
            ? "border-foreground bg-foreground text-background"
            : "border-border bg-surface hover:bg-background",
        )}
      >
        Series
      </Link>
    </div>
  );
}

function SeriesGrid({
  series,
  artistSlug,
}: {
  series: PublicSeries[];
  artistSlug: string;
}) {
  if (series.length === 0) {
    return (
      <p className="text-muted text-sm">
        No series yet — switch back to{" "}
        <Link
          href={`/artists/${artistSlug}`}
          className="underline hover:text-foreground"
        >
          Works
        </Link>{" "}
        to see this artist&apos;s pieces.
      </p>
    );
  }
  return (
    <ul className="grid grid-cols-1 sm:grid-cols-2 md:grid-cols-3 gap-6">
      {series.map((s) => (
        <li key={s.id}>
          <Link
            href={`/artists/${artistSlug}/series/${s.slug}`}
            className="block bg-surface border border-border hover:opacity-95"
          >
            <div className="relative aspect-square bg-background overflow-hidden">
              {s.cover_image_url ? (
                // eslint-disable-next-line @next/next/no-img-element
                <img
                  src={s.cover_image_url}
                  alt={s.title}
                  loading="lazy"
                  className="w-full h-full object-cover"
                />
              ) : (
                <div className="w-full h-full flex items-center justify-center text-muted text-xs">
                  No cover image
                </div>
              )}
            </div>
            <div className="p-3">
              <h3 className="font-serif text-base line-clamp-1">{s.title}</h3>
              <p className="text-xs text-muted mt-1">
                {s.artwork_count}{" "}
                {s.artwork_count === 1 ? "work" : "works"}
              </p>
              {s.statement && (
                <p className="text-sm text-muted mt-2 line-clamp-2">
                  {s.statement}
                </p>
              )}
            </div>
          </Link>
        </li>
      ))}
    </ul>
  );
}

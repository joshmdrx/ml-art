import type { Metadata } from "next";
import { auth } from "@clerk/nextjs/server";
import Link from "next/link";
import { TopNav } from "@/components/TopNav";
import { PreviewActions } from "@/components/PreviewActions";
import { PreviewGone } from "@/components/PreviewGone";
import { ArtistLocationsMap } from "@/components/ArtistLocationsMap";
import { getDraftPreview, getMe, type ArtworkSummary } from "@/lib/api";
import { formatPrice } from "@/lib/format";
import { reportError } from "@/lib/reportError";

/**
 * T-086 — `/preview/[token]`: a private example page Wander built for
 * a gallery or studio (decisions.md 2026-11-15, 2026-11-26). The
 * token is the only credential. From here they can claim the page
 * (sign in, it becomes theirs, still unpublished) or remove it.
 *
 * Never indexed: `robots` metadata here plus a `/preview` disallow in
 * `app/robots.ts`. Laid out like `/artists/[slug]` so it shows what the
 * live page would be. Artworks open the private artwork page under this
 * token (their public pages don't exist yet), and the map hides its
 * "See on full map" link because a draft has no pins on the public map.
 */

type Params = Promise<{ token: string }>;

export const metadata: Metadata = {
  title: "Private preview",
  robots: { index: false, follow: false, nocache: true },
  referrer: "no-referrer",
};

export default async function PreviewPage({ params }: { params: Params }) {
  const { token } = await params;
  const [data, session, me] = await Promise.all([
    getDraftPreview(token).catch((e) => {
      reportError(e, { surface: "draft-preview" });
      return null;
    }),
    auth(),
    getMe().catch(() => null),
  ]);

  if (!data) return <PreviewGone />;

  const { artist, artworks, locations } = data;
  const expires = new Date(data.expires_at).toLocaleDateString("en-GB", {
    day: "numeric",
    month: "long",
    year: "numeric",
  });

  return (
    <>
      <TopNav />
      <main className="flex-1 mx-auto w-full max-w-screen-2xl px-6 py-12 md:py-16">
        <section
          aria-label="About this preview"
          className="mb-12 max-w-3xl border border-foreground bg-surface p-5"
        >
          <p className="text-xs uppercase tracking-wide text-muted">Private preview</p>
          <h2 className="mt-1 font-serif text-2xl">
            An example Wander page for {artist.display_name}
          </h2>
          <p className="mt-3 text-sm leading-relaxed">
            We put this together from your public website to show you
            what a Wander page could look like. It isn&apos;t listed
            anywhere: only people
            with this link can open it, and the link stops working on{" "}
            {expires}. Listing on Wander is always free.
          </p>
          <ul className="mt-3 text-sm leading-relaxed list-disc pl-5 space-y-1">
            <li>
              <strong>Claim it</strong> to make it yours. You can edit
              everything, and it only goes live when you choose to
              publish.
            </li>
            <li>
              <strong>Remove it</strong>{" "}
              if you&apos;d rather not. The
              draft is deleted and this link stops working.
            </li>
          </ul>
          <PreviewActions
            token={token}
            name={artist.display_name}
            signedIn={Boolean(session.userId)}
            isAdmin={Boolean(me?.is_admin)}
          />
        </section>

        <header className="max-w-3xl mb-12 md:mb-16">
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
          {artist.bio && (
            <p className="mt-6 text-base leading-relaxed max-w-2xl">{artist.bio}</p>
          )}
          {artist.website_url && (
            <p className="mt-6 text-sm">
              <a
                href={artist.website_url}
                target="_blank"
                rel="noopener noreferrer"
                className="underline hover:no-underline"
              >
                Website ↗
              </a>
            </p>
          )}
        </header>

        <ArtistLocationsMap
          locations={locations}
          artistSlug={artist.slug}
          showFullMapLink={false}
        />

        {artist.artist_statement && (
          <section className="max-w-2xl mb-12 md:mb-16">
            <h2 className="font-serif text-xl mb-3">
              {artist.entity_type === "gallery" ? "Curatorial statement" : "Artist statement"}
            </h2>
            <p className="text-base leading-relaxed whitespace-pre-line">
              {artist.artist_statement}
            </p>
          </section>
        )}

        <section>
          <h2 className="font-serif text-xl mb-6">Works</h2>
          {artworks.length === 0 ? (
            <p className="text-muted text-sm">No works added to this example yet.</p>
          ) : (
            <PreviewArtworkGrid items={artworks} token={token} />
          )}
        </section>
      </main>
    </>
  );
}

/** Same look as `ArtworkGrid` / `ArtworkCard`, linking to the private
 * artwork page instead of `/artworks/[id]`. No "more like this" tray:
 * it searches published work, which a draft isn't part of. */
function PreviewArtworkGrid({
  items,
  token,
}: {
  items: ArtworkSummary[];
  token: string;
}) {
  return (
    <div className="grid grid-cols-2 md:grid-cols-3 lg:grid-cols-4 gap-6 md:gap-8">
      {items.map((a) => {
        const href = `/preview/${token}/artworks/${a.id}`;
        const title = a.title ?? "Untitled";
        const price = formatPrice(a.price_cents, a.currency);
        return (
          <div key={a.id} className="group">
            <Link
              href={href}
              aria-label={`${title} by ${a.artist_name}`}
              className="block bg-surface border border-border overflow-hidden"
            >
              {a.primary_image_url ? (
                // eslint-disable-next-line @next/next/no-img-element
                <img
                  src={a.primary_image_url}
                  alt={`${title} by ${a.artist_name}`}
                  loading="lazy"
                  className="w-full h-auto block transition-opacity group-hover:opacity-95"
                />
              ) : (
                <div className="aspect-square bg-border" />
              )}
            </Link>
            <div className="mt-2 px-0.5 min-w-0">
              <p className="text-sm text-muted truncate">{a.artist_name}</p>
              <Link
                href={href}
                className="text-sm font-serif hover:underline truncate block"
              >
                {title}
              </Link>
              {price && <div className="text-xs text-muted mt-0.5">{price}</div>}
            </div>
          </div>
        );
      })}
    </div>
  );
}

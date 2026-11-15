import type { Metadata } from "next";
import Link from "next/link";
import { auth } from "@clerk/nextjs/server";
import { TopNav } from "@/components/TopNav";
import { PreviewActions } from "@/components/PreviewActions";
import { getDraftPreview, type ArtworkSummary } from "@/lib/api";
import { reportError } from "@/lib/reportError";

/**
 * T-086 — `/preview/[token]`: a private example page Wander built for
 * a gallery or studio after they asked for one. The token is the only
 * credential. From here they can claim the page (sign in, it becomes
 * theirs, still unpublished) or remove it.
 *
 * Never indexed: `robots` metadata here plus a `/preview` disallow in
 * `app/robots.ts`. Artworks render without links because their public
 * pages don't exist yet; locations render as a plain list for the same
 * reason (the map's "see on full map" link would lead nowhere).
 */

type Params = Promise<{ token: string }>;

export const metadata: Metadata = {
  title: "Private preview",
  robots: { index: false, follow: false, nocache: true },
  referrer: "no-referrer",
};

export default async function PreviewPage({ params }: { params: Params }) {
  const { token } = await params;
  const [data, session] = await Promise.all([
    getDraftPreview(token).catch((e) => {
      reportError(e, { surface: "draft-preview" });
      return null;
    }),
    auth(),
  ]);

  if (!data) {
    return (
      <>
        <TopNav />
        <main className="flex-1 mx-auto w-full max-w-2xl px-6 py-20">
          <h1 className="font-serif text-3xl tracking-tight">
            This preview link no longer works
          </h1>
          <p className="mt-4 text-muted">
            It may have expired, been replaced with a newer link, been
            claimed, or been removed. If you were expecting to see a
            draft page, reply to the email it came in and we&apos;ll
            sort it out.
          </p>
          <p className="mt-8 text-sm">
            <Link href="/" className="underline hover:no-underline">
              Go to Wander
            </Link>
          </p>
        </main>
      </>
    );
  }

  const { artist, artworks, locations } = data;
  const expires = new Date(data.expires_at).toLocaleDateString("en-GB", {
    day: "numeric",
    month: "long",
    year: "numeric",
  });

  return (
    <>
      <TopNav />
      <main className="flex-1 mx-auto w-full max-w-screen-2xl px-6 py-10 md:py-14">
        <section
          aria-label="About this preview"
          className="mb-12 max-w-3xl border border-foreground bg-surface p-5"
        >
          <p className="text-xs uppercase tracking-wide text-muted">Private preview</p>
          <h2 className="mt-1 font-serif text-2xl">
            An example Wander page for {artist.display_name}
          </h2>
          <p className="mt-3 text-sm leading-relaxed">
            We put this together from your public website because you
            asked to see one. It isn&apos;t listed anywhere: only people
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
          />
        </section>

        <header className="max-w-3xl mb-12">
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

        {locations.length > 0 && (
          <section className="max-w-2xl mb-12">
            <h2 className="font-serif text-xl mb-3">Where to see the work</h2>
            <ul className="space-y-2 text-sm">
              {locations.map((l) => (
                <li key={l.id}>
                  <span className="font-medium">{l.name}</span>
                  <span className="text-muted"> · {l.address}</span>
                </li>
              ))}
            </ul>
            <p className="mt-2 text-xs text-muted">
              On the live page these appear on a map.
            </p>
          </section>
        )}

        {artist.artist_statement && (
          <section className="max-w-2xl mb-12">
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
            <PreviewArtworkGrid items={artworks} />
          )}
        </section>
      </main>
    </>
  );
}

function PreviewArtworkGrid({ items }: { items: ArtworkSummary[] }) {
  return (
    <ul className="grid grid-cols-2 md:grid-cols-3 lg:grid-cols-4 gap-6">
      {items.map((a) => (
        <li key={a.id}>
          <div className="relative aspect-square bg-surface border border-border overflow-hidden">
            {a.primary_image_url ? (
              // eslint-disable-next-line @next/next/no-img-element
              <img
                src={a.primary_image_url}
                alt={a.title ?? "Untitled"}
                loading="lazy"
                className="w-full h-full object-cover"
              />
            ) : (
              <div className="w-full h-full flex items-center justify-center text-muted text-xs">
                Image pending
              </div>
            )}
          </div>
          <p className="mt-2 font-serif text-sm line-clamp-1">{a.title ?? "Untitled"}</p>
        </li>
      ))}
    </ul>
  );
}

import type { Metadata } from "next";
import Link from "next/link";
import { TopNav } from "@/components/TopNav";
import { ArtworkImageViewer } from "@/components/ArtworkImageViewer";
import { ArtworkFacts } from "@/components/ArtworkFacts";
import { PreviewGone } from "@/components/PreviewGone";
import { getDraftPreviewArtwork } from "@/lib/api";
import { reportError } from "@/lib/reportError";

/**
 * T-086 — `/preview/[token]/artworks/[id]`: one artwork on a private
 * example page, laid out like `/artworks/[id]` so the organisation sees
 * what a visitor would. The token is the credential, same as the
 * example page; the API answers 404 unless the artwork is on that draft.
 *
 * Enquire / Buy / Save and "more like this" are left out: they act on
 * public artworks, and this one isn't public. A note says what the live
 * page adds instead. Never indexed (metadata here, `/preview` disallowed
 * in `app/robots.ts`).
 */

type Params = Promise<{ token: string; id: string }>;

export const metadata: Metadata = {
  title: "Private preview",
  robots: { index: false, follow: false, nocache: true },
  referrer: "no-referrer",
};

export default async function PreviewArtworkPage({ params }: { params: Params }) {
  const { token, id } = await params;
  const artwork = await getDraftPreviewArtwork(token, id).catch((e) => {
    reportError(e, { surface: "draft-preview-artwork" });
    return null;
  });
  if (!artwork) return <PreviewGone />;

  const back = `/preview/${token}`;

  return (
    <>
      <TopNav />

      <main className="flex-1 mx-auto w-full max-w-screen-2xl px-6 py-10 md:py-12">
        <p className="mb-8 text-sm">
          <span className="text-xs uppercase tracking-wide text-muted mr-3">
            Private preview
          </span>
          <Link href={back} className="underline hover:no-underline">
            ← Back to the example page
          </Link>
        </p>

        <article className="grid grid-cols-1 md:grid-cols-12 gap-8 md:gap-12">
          <div className="md:col-span-7 lg:col-span-8">
            <ArtworkImageViewer
              images={artwork.images}
              title={artwork.title}
              artistName={artwork.artist.display_name}
            />
          </div>

          <aside className="md:col-span-5 lg:col-span-4">
            <h1 className="font-serif text-3xl md:text-4xl tracking-tight">
              {artwork.title ?? "Untitled"}
            </h1>

            <p className="mt-2 text-sm">
              by{" "}
              <Link
                href={back}
                className="underline underline-offset-2 hover:no-underline"
              >
                {artwork.artist.display_name}
              </Link>
              {artwork.artist.location && (
                <span className="text-muted"> · {artwork.artist.location}</span>
              )}
            </p>

            <ArtworkFacts artwork={artwork} />

            {artwork.description && (
              <p className="mt-8 text-sm leading-relaxed whitespace-pre-line">
                {artwork.description}
              </p>
            )}

            <p className="mt-8 border border-border bg-surface p-4 text-sm text-muted leading-relaxed">
              On the live page, visitors can enquire about this work
              (the message comes to you, or links to your own site),
              save it, and find similar work nearby. If you turn on
              direct sales, they can buy it here too.
            </p>
          </aside>
        </article>
      </main>
    </>
  );
}

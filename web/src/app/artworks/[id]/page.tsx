import { notFound } from "next/navigation";
import type { Metadata } from "next";
import Link from "next/link";
import { TopNav } from "@/components/TopNav";
import { ArtworkGrid } from "@/components/ArtworkGrid";
import { ArtworkImageViewer } from "@/components/ArtworkImageViewer";
import { BackToSearchLink } from "@/components/BackToSearchLink";
import { InquireButton } from "@/components/InquireButton";
import { SaveButton } from "@/components/SaveButton";
import { formatMedium } from "@/lib/medium";
import {
  getArtwork,
  getSimilarArtworks,
  formatPrice,
  formatDimensions,
} from "@/lib/api";
import { reportError } from "@/lib/reportError";

/**
 * /artworks/[id] — artwork detail (full page).
 *
 * Modal-overlay variant (intercepting routes) deferred per
 * `decisions.md` 2026-05-25 (artwork detail: full-page first).
 */

type Params = Promise<{ id: string }>;

export async function generateMetadata({
  params,
}: {
  params: Params;
}): Promise<Metadata> {
  const { id } = await params;
  const a = await getArtwork(id).catch(() => null);
  if (!a) return { title: "Artwork not found" };
  const title = a.title ?? "Untitled";
  return {
    title: `${title} by ${a.artist.display_name}`,
    description:
      a.description?.slice(0, 160) ?? `${title} by ${a.artist.display_name}`,
  };
}

export default async function ArtworkPage({ params }: { params: Params }) {
  const { id } = await params;

  const [artwork, similar] = await Promise.all([
    getArtwork(id).catch((e) => {
      reportError(e, { surface: "artwork-detail", id });
      return null;
    }),
    getSimilarArtworks(id, { limit: 8 }).catch((e) => {
      reportError(e, { surface: "artwork-similar", id });
      return { items: [], next_cursor: null };
    }),
  ]);

  if (!artwork) notFound();

  const price = formatPrice(artwork.price_cents, artwork.currency);
  const dims = formatDimensions(artwork.dimensions);
  const availabilityLabel = labelForAvailability(artwork.availability);

  return (
    <>
      <TopNav />

      <main className="flex-1 mx-auto w-full max-w-screen-2xl px-6 py-10 md:py-12">
        {/* Detail grid: image (left), details (right) on desktop; stacked on mobile. */}
        <article className="grid grid-cols-1 md:grid-cols-12 gap-8 md:gap-12">
          {/* Image column */}
          <div className="md:col-span-7 lg:col-span-8">
            <ArtworkImageViewer
              images={artwork.images}
              title={artwork.title}
              artistName={artwork.artist.display_name}
            />
          </div>

          {/* Details column */}
          <aside className="md:col-span-5 lg:col-span-4">
            <h1 className="font-serif text-3xl md:text-4xl tracking-tight">
              {artwork.title ?? "Untitled"}
            </h1>

            <p className="mt-2 text-sm">
              by{" "}
              <Link
                href={`/artists/${artwork.artist.slug}`}
                className="underline underline-offset-2 hover:no-underline"
              >
                {artwork.artist.display_name}
              </Link>
              {artwork.artist.location && (
                <span className="text-muted"> · {artwork.artist.location}</span>
              )}
            </p>

            <dl className="mt-8 space-y-3 text-sm">
              {artwork.year_created && (
                <Row label="Year" value={String(artwork.year_created)} />
              )}
              {/* T-073 — combiner renders "Painting · Oil on linen"
                  if both, the category Title Case if only category,
                  the raw materials text if only that (legacy). */}
              {(() => {
                const v = formatMedium(artwork.medium_category, artwork.medium);
                return v ? <Row label="Medium" value={v} /> : null;
              })()}
              {dims && <Row label="Dimensions" value={dims} />}
              <Row
                label="Availability"
                value={availabilityLabel}
                muted={artwork.availability === "sold"}
              />
              <Row label="Price" value={price ?? "On inquiry"} />
            </dl>

            {artwork.description && (
              <p className="mt-8 text-sm leading-relaxed whitespace-pre-line">
                {artwork.description}
              </p>
            )}

            <div className="mt-8 flex flex-col gap-3">
              <InquireButton
                artworkId={artwork.id}
                artistName={artwork.artist.display_name}
              />
              <SaveButton artworkId={artwork.id} />
              {/* Routes the user through the full search surface
                  (filters + modifiers + map) using this artwork's
                  embedding as the visual anchor. The dedicated
                  "More like this" section below already shows a
                  top-N — this CTA is for "I want to *explore* from
                  this image". */}
              <Link
                href={`/search?seed_artwork_id=${encodeURIComponent(artwork.id)}`}
                className="text-sm text-center border border-border bg-surface hover:bg-background px-4 py-2 transition-colors"
              >
                Find visually similar →
              </Link>
            </div>
          </aside>
        </article>

        {/* More like this */}
        {similar.items.length > 0 && (
          <section className="mt-20">
            <h2 className="font-serif text-xl mb-6">More like this</h2>
            <ArtworkGrid items={similar.items} />
          </section>
        )}

        <p className="mt-16 text-xs text-muted">
          <BackToSearchLink className="hover:text-foreground" />
        </p>
      </main>
    </>
  );
}

function Row({
  label,
  value,
  muted,
}: {
  label: string;
  value: string;
  muted?: boolean;
}) {
  return (
    <div className="flex items-baseline gap-4">
      <dt className="w-24 shrink-0 text-muted">{label}</dt>
      <dd className={muted ? "text-muted" : ""}>{value}</dd>
    </div>
  );
}

function labelForAvailability(a: string): string {
  switch (a) {
    case "available":
      return "Available";
    case "sold":
      return "Sold";
    case "not_for_sale":
      return "Not for sale";
    case "inquire":
      return "On inquiry";
    default:
      return a;
  }
}

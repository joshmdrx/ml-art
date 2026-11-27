import type { ArtworkFull } from "@/lib/api";
import { formatDimensions, formatEdition, formatPrice } from "@/lib/format";
import { formatMedium } from "@/lib/medium";

/**
 * The year / medium / dimensions / edition / availability / price list
 * on an artwork's detail page. Shared by `/artworks/[id]` and the
 * private preview's artwork page (T-086) so the two can't drift.
 */
export function ArtworkFacts({ artwork }: { artwork: ArtworkFull }) {
  const price = formatPrice(artwork.price_cents, artwork.currency);
  const dims = formatDimensions(artwork.dimensions);
  const availabilityLabel =
    artwork.edition_size != null && artwork.availability === "sold"
      ? "Sold out"
      : labelForAvailability(artwork.availability);
  const edition = formatEdition(artwork.edition_size, artwork.editions_remaining);
  // T-073 — combiner renders "Painting · Oil on linen" if both, the
  // category Title Case if only category, the raw materials text if
  // only that (legacy).
  const medium = formatMedium(artwork.medium_category, artwork.medium);

  return (
    <dl className="mt-8 space-y-3 text-sm">
      {artwork.year_created && (
        <Row label="Year" value={String(artwork.year_created)} />
      )}
      {medium && <Row label="Medium" value={medium} />}
      {dims && <Row label="Dimensions" value={dims} />}
      {edition && <Row label="Edition" value={edition} />}
      <Row
        label="Availability"
        value={availabilityLabel}
        muted={artwork.availability === "sold"}
      />
      <Row label="Price" value={price ?? "On inquiry"} />
    </dl>
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

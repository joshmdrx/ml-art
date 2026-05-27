import Link from "next/link";
import type { Neighborhood } from "@/lib/api";

/**
 * Asymmetric 3-thumbnail layout per `01-page-spec.md`:
 * one tall image on the left, two stacked smaller ones on the right.
 * Falls back to whatever images are available; missing ones show as
 * empty bordered boxes.
 */
export function NeighborhoodCard({
  neighborhood,
}: {
  neighborhood: Neighborhood;
}) {
  const [a, b, c] = [
    neighborhood.representative_image_urls[0],
    neighborhood.representative_image_urls[1],
    neighborhood.representative_image_urls[2],
  ];

  return (
    <Link
      href={`/neighborhoods/${neighborhood.slug}`}
      className="group block bg-surface border border-border p-4 transition-colors hover:border-foreground/30"
    >
      <div className="grid grid-cols-3 grid-rows-2 gap-2 h-48 mb-4">
        <Thumb src={a} className="row-span-2 col-span-2" alt="" />
        <Thumb src={b} alt="" />
        <Thumb src={c} alt="" />
      </div>
      <h3 className="font-serif text-lg leading-tight">{neighborhood.name}</h3>
      {neighborhood.description && (
        <p className="mt-1 text-xs text-muted line-clamp-2">
          {neighborhood.description}
        </p>
      )}
      <p className="mt-2 text-xs text-muted">
        {neighborhood.artwork_count} works
      </p>
    </Link>
  );
}

function Thumb({
  src,
  className = "",
  alt,
}: {
  src: string | undefined;
  className?: string;
  alt: string;
}) {
  return (
    <div
      className={`bg-border overflow-hidden ${className}`}
      aria-hidden={!src}
    >
      {src && (
        // eslint-disable-next-line @next/next/no-img-element
        <img
          src={src}
          alt={alt}
          loading="lazy"
          className="w-full h-full object-cover"
        />
      )}
    </div>
  );
}

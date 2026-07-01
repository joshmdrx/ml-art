"use client";

/**
 * Artwork detail image viewer.
 *
 * Three affordances:
 *   - Main image is capped at ~viewport height so the whole piece is
 *     visible without scrolling (previous behaviour let tall works
 *     overflow the fold).
 *   - Click the main image → full-screen lightbox at up to 95vw/95vh.
 *     Close via × / Escape / backdrop click.
 *   - Thumbnails below the main viewer for multi-image works;
 *     clicking a thumbnail swaps it into the primary slot. The strip
 *     is hidden when the artwork has only one image (no useful
 *     interaction).
 *
 * The initial selection prefers the `is_primary` image; if the
 * backend ever returns rows without a primary flag we fall back to
 * the first image in the array. Images come pre-sorted by the API
 * (`is_primary DESC, display_order ASC`).
 */

import { useState } from "react";
import * as Dialog from "@radix-ui/react-dialog";
import { clsx } from "clsx";
import type { ArtworkImage } from "@/lib/api";

export function ArtworkImageViewer({
  images,
  title,
  artistName,
}: {
  images: ArtworkImage[];
  title: string | null;
  artistName: string;
}) {
  // Prefer the primary; fall back to the first row if none is flagged.
  const initialIdx = Math.max(
    0,
    images.findIndex((i) => i.is_primary),
  );
  const [selectedIdx, setSelectedIdx] = useState(initialIdx);
  const [lightboxOpen, setLightboxOpen] = useState(false);

  if (images.length === 0) {
    return <div className="aspect-square bg-border" />;
  }

  const current = images[selectedIdx] ?? images[0];
  const alt = title
    ? `${title} by ${artistName}`
    : `Untitled by ${artistName}`;

  return (
    <>
      {/* Main image — fit to viewport height, click to zoom. Wrapper
          <button> covers empty space around portrait images so the
          click target is the whole frame, not just the image bitmap. */}
      <button
        type="button"
        onClick={() => setLightboxOpen(true)}
        className="block w-full bg-surface border border-border cursor-zoom-in"
        aria-label="Open image in full view"
      >
        <div className="flex items-center justify-center">
          {/* eslint-disable-next-line @next/next/no-img-element */}
          <img
            src={current.url}
            alt={alt}
            className="max-w-full max-h-[calc(100vh-8rem)] object-contain"
          />
        </div>
      </button>

      {images.length > 1 && (
        <div
          className="mt-4 flex gap-3 overflow-x-auto"
          role="tablist"
          aria-label="Artwork images"
        >
          {images.map((im, i) => (
            <button
              key={im.id}
              type="button"
              role="tab"
              aria-selected={i === selectedIdx}
              aria-label={`Show image ${i + 1} of ${images.length}`}
              onClick={() => setSelectedIdx(i)}
              className={clsx(
                "shrink-0 border transition-colors bg-surface",
                i === selectedIdx
                  ? "border-foreground"
                  : "border-border hover:border-foreground/60",
              )}
            >
              {/* eslint-disable-next-line @next/next/no-img-element */}
              <img
                src={im.url}
                alt=""
                loading="lazy"
                className="h-24 w-24 object-cover"
              />
            </button>
          ))}
        </div>
      )}

      <Dialog.Root open={lightboxOpen} onOpenChange={setLightboxOpen}>
        <Dialog.Portal>
          <Dialog.Overlay className="fixed inset-0 bg-foreground/95 z-40" />
          <Dialog.Content
            className="fixed inset-0 z-50 flex items-center justify-center p-4 outline-none"
            aria-describedby={undefined}
          >
            <Dialog.Title className="sr-only">{alt}</Dialog.Title>
            {/* eslint-disable-next-line @next/next/no-img-element */}
            <img
              src={current.url}
              alt={alt}
              className="max-w-[95vw] max-h-[95vh] object-contain"
            />
            <Dialog.Close asChild>
              <button
                type="button"
                aria-label="Close full view"
                className="absolute top-4 right-4 text-background hover:opacity-80 text-4xl leading-none w-10 h-10 flex items-center justify-center"
              >
                ×
              </button>
            </Dialog.Close>
          </Dialog.Content>
        </Dialog.Portal>
      </Dialog.Root>
    </>
  );
}

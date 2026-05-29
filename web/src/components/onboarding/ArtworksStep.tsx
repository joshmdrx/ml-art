"use client";

/**
 * T-012 Phase 1 — onboarding step 3: add your first artworks.
 *
 * Reuses the `ArtworkEditModal` from the studio surface so the create
 * flow (title, medium, dimensions, price, image upload) is exactly the
 * same code the studio uses. The wizard's job is just to put it in
 * front of the artist + nudge them to add at least one before
 * continuing.
 *
 * "Continue" is enabled with zero artworks — we don't block on having
 * a portfolio. A first-time artist can publish empty and add work
 * later via the studio.
 */

import { useState } from "react";
import Link from "next/link";
import { useRouter } from "next/navigation";
import { ArtworkEditModal } from "@/components/ArtworkEditModal";
import type { StudioArtist, StudioArtworkSummary } from "@/lib/api";

interface Props {
  artist: StudioArtist;
  items: StudioArtworkSummary[];
}

export function ArtworksStep({ artist, items }: Props) {
  const router = useRouter();
  const [open, setOpen] = useState<string | "new" | null>(null);

  function closeModal() {
    setOpen(null);
    // Refresh to pick up newly-created artworks.
    router.refresh();
  }

  return (
    <section className="space-y-6 max-w-3xl">
      <div>
        <h2 className="font-serif text-2xl tracking-tight">
          Add a few artworks
        </h2>
        <p className="mt-2 text-sm text-muted">
          Two or three is plenty to get going — you can add more later from
          your studio. Skip this step if you&apos;d rather come back to it.
        </p>
      </div>

      {items.length === 0 ? (
        <div className="border border-dashed border-border bg-surface p-8 text-center">
          <p className="text-sm text-muted">No artworks yet.</p>
          <button
            type="button"
            onClick={() => setOpen("new")}
            className="mt-4 text-sm px-4 py-2 bg-fg text-bg"
          >
            + Add your first artwork
          </button>
        </div>
      ) : (
        <>
          <ul className="grid grid-cols-2 md:grid-cols-3 gap-4">
            {items.map((a) => (
              <li
                key={a.id}
                className="border border-border bg-surface"
              >
                <button
                  type="button"
                  onClick={() => setOpen(a.id)}
                  className="block w-full text-left"
                  aria-label={`Edit ${a.title ?? "untitled artwork"}`}
                >
                  <div className="aspect-square bg-bg overflow-hidden">
                    {a.primary_image_url ? (
                      // eslint-disable-next-line @next/next/no-img-element
                      <img
                        src={a.primary_image_url}
                        alt=""
                        className="w-full h-full object-cover"
                      />
                    ) : (
                      <div className="w-full h-full flex items-center justify-center text-xs text-muted">
                        No image
                      </div>
                    )}
                  </div>
                  <div className="p-3">
                    <p className="text-sm font-medium truncate">
                      {a.title ?? "Untitled"}
                    </p>
                    <p className="text-[10px] uppercase tracking-wider text-muted mt-1">
                      {a.status}
                    </p>
                  </div>
                </button>
              </li>
            ))}
          </ul>
          <button
            type="button"
            onClick={() => setOpen("new")}
            className="text-sm px-4 py-2 border border-border hover:bg-surface"
          >
            + Add another artwork
          </button>
        </>
      )}

      <div className="flex items-center justify-between pt-4 border-t border-border">
        <Link
          href="/onboarding?step=profile"
          className="text-sm text-muted hover:text-foreground"
        >
          ← Back
        </Link>
        <Link
          href="/onboarding?step=locations"
          className="text-sm px-4 py-2 bg-fg text-bg"
        >
          Continue
        </Link>
      </div>

      <ArtworkEditModal
        artistDisplayName={artist.display_name}
        open={open !== null}
        target={open}
        onClose={closeModal}
      />
    </section>
  );
}

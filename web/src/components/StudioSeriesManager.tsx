"use client";

/**
 * T-058.2 — studio series management.
 *
 * Reads server-fetched series + artworks lists; renders a grid of
 * series cards + the "+ New series" affordance. Opens
 * `SeriesEditModal` for create (`"new"`) and edit (`<uuid>`).
 *
 * Optimistic local state: edits / creates / deletes update the local
 * `series` array immediately on success so the user sees the change
 * without a full refresh. `router.refresh()` runs on modal close to
 * re-sync with the server.
 */

import { useCallback, useState } from "react";
import { useRouter } from "next/navigation";
import { SeriesEditModal } from "@/components/SeriesEditModal";
import type { StudioArtist, StudioArtworkSummary, StudioSeries } from "@/lib/api";

interface Props {
  artist: StudioArtist;
  initialSeries: StudioSeries[];
  artworks: StudioArtworkSummary[];
}

type ModalTarget = StudioSeries | "new" | null;

export function StudioSeriesManager({
  artist,
  initialSeries,
  artworks,
}: Props) {
  const router = useRouter();
  const [series, setSeries] = useState<StudioSeries[]>(initialSeries);
  const [open, setOpen] = useState<ModalTarget>(null);

  const onSaved = useCallback((updated: StudioSeries) => {
    setSeries((prev) => {
      const idx = prev.findIndex((s) => s.id === updated.id);
      if (idx === -1) return [updated, ...prev];
      const next = prev.slice();
      next[idx] = updated;
      return next;
    });
  }, []);

  const onDeleted = useCallback((id: string) => {
    setSeries((prev) => prev.filter((s) => s.id !== id));
  }, []);

  const closeModal = useCallback(() => {
    setOpen(null);
    router.refresh();
  }, [router]);

  return (
    <>
      <div className="flex justify-end mb-6">
        <button
          type="button"
          onClick={() => setOpen("new")}
          className="px-4 py-2 text-sm bg-foreground text-background"
        >
          + New series
        </button>
      </div>

      {series.length === 0 ? (
        <div className="border border-dashed border-border p-12 text-center">
          <p className="text-sm text-muted mb-4">
            No series yet. Group works that belong together — a project, a
            year, a theme.
          </p>
          <button
            type="button"
            onClick={() => setOpen("new")}
            className="text-sm underline underline-offset-2"
          >
            Create your first series
          </button>
        </div>
      ) : (
        <ul className="grid grid-cols-1 sm:grid-cols-2 md:grid-cols-3 lg:grid-cols-4 gap-4">
          {series.map((s) => (
            <li key={s.id}>
              <SeriesCard series={s} onEdit={() => setOpen(s)} />
            </li>
          ))}
        </ul>
      )}

      <SeriesEditModal
        open={open !== null}
        target={open}
        artworks={artworks}
        artistName={artist.display_name}
        onSaved={onSaved}
        onDeleted={onDeleted}
        onClose={closeModal}
      />
    </>
  );
}

function SeriesCard({
  series,
  onEdit,
}: {
  series: StudioSeries;
  onEdit: () => void;
}) {
  return (
    <article className="border border-border bg-surface">
      <button
        type="button"
        onClick={onEdit}
        className="block w-full text-left hover:opacity-95 focus:outline-none focus-visible:ring-2 focus-visible:ring-foreground"
      >
        <div className="relative aspect-square bg-background overflow-hidden">
          {series.cover_image_url ? (
            // eslint-disable-next-line @next/next/no-img-element
            <img
              src={series.cover_image_url}
              alt={series.title}
              loading="lazy"
              className="w-full h-full object-cover"
            />
          ) : (
            <div className="w-full h-full flex items-center justify-center text-muted text-xs">
              No artworks yet
            </div>
          )}
        </div>
        <div className="p-3">
          <h3 className="font-serif text-base line-clamp-1">{series.title}</h3>
          <p className="text-xs text-muted mt-1">
            {series.artwork_count}{" "}
            {series.artwork_count === 1 ? "work" : "works"}
          </p>
        </div>
      </button>
    </article>
  );
}

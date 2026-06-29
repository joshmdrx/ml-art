"use client";

/**
 * T-058.2 — studio series management.
 *
 * Modal lifecycle is driven by URL params (`?id=` + `?tab=`) rather
 * than local state. Benefits:
 *
 * - Shareable links: `/studio/series?id=<uuid>&tab=artworks` opens a
 *   specific series on the membership tab. Browser back button closes
 *   the modal naturally.
 * - Refresh-friendly: reload doesn't bounce you out of the modal.
 * - Multi-step flows fall out of routing: after a create, the parent
 *   does `router.replace("?id=<new>&tab=artworks")` and the modal
 *   re-renders for the new series on the right tab. No useRef "did
 *   the parent just re-open us?" detection needed.
 *
 * Optimistic local state: edits / creates / deletes update the local
 * `series` array immediately on success so the user sees the change
 * without a full route refresh. `router.refresh()` runs on close
 * (cleanup) to reconcile with server.
 */

import { useCallback, useMemo, useState } from "react";
import { useRouter, useSearchParams } from "next/navigation";
import { SeriesEditModal } from "@/components/SeriesEditModal";
import type {
  StudioArtist,
  StudioArtworkSummary,
  StudioSeries,
} from "@/lib/api";

interface Props {
  artist: StudioArtist;
  initialSeries: StudioSeries[];
  artworks: StudioArtworkSummary[];
}

type Tab = "details" | "artworks";

export function StudioSeriesManager({
  artist,
  initialSeries,
  artworks,
}: Props) {
  const router = useRouter();
  const searchParams = useSearchParams();
  const [series, setSeries] = useState<StudioSeries[]>(initialSeries);

  const idParam = searchParams.get("id"); // "new" | uuid | null
  const tabParam = searchParams.get("tab");
  const tab: Tab = tabParam === "artworks" ? "artworks" : "details";

  // Resolve `?id=<uuid>` into the actual series row from the local
  // list. "new" is a sentinel (no row to look up). Anything else
  // (null / unrecognised id) → modal closed.
  const target: StudioSeries | "new" | null = useMemo(() => {
    if (idParam === "new") return "new";
    if (!idParam) return null;
    return series.find((s) => s.id === idParam) ?? null;
  }, [idParam, series]);

  const openCreate = useCallback(() => {
    router.replace("/studio/series?id=new&tab=details", { scroll: false });
  }, [router]);

  const openEdit = useCallback(
    (id: string) => {
      router.replace(
        `/studio/series?id=${encodeURIComponent(id)}&tab=details`,
        { scroll: false },
      );
    },
    [router],
  );

  const setTab = useCallback(
    (next: Tab) => {
      if (!idParam) return;
      router.replace(
        `/studio/series?id=${encodeURIComponent(idParam)}&tab=${next}`,
        { scroll: false },
      );
    },
    [idParam, router],
  );

  const closeModal = useCallback(() => {
    router.replace("/studio/series", { scroll: false });
    // Reconcile any optimistic local state with the server on close.
    router.refresh();
  }, [router]);

  const onSaved = useCallback(
    (updated: StudioSeries) => {
      setSeries((prev) => {
        const idx = prev.findIndex((s) => s.id === updated.id);
        if (idx === -1) return [updated, ...prev];
        const next = prev.slice();
        next[idx] = updated;
        return next;
      });
      // Multi-step flow: after a successful create, advance to the
      // newly-created series on the Artworks tab so the artist can
      // attach work in the same flow. URL-driven so the transition
      // is a single `router.replace` — no in-modal state machine. Per
      // docs/ui-patterns.md → Modal / dialog behaviour.
      // Edit-mode saves close: the artist clicked Save to commit,
      // not to keep editing.
      if (idParam === "new") {
        router.replace(
          `/studio/series?id=${encodeURIComponent(updated.id)}&tab=artworks`,
          { scroll: false },
        );
      } else {
        closeModal();
      }
    },
    [idParam, router, closeModal],
  );

  const onDeleted = useCallback(
    (id: string) => {
      setSeries((prev) => prev.filter((s) => s.id !== id));
      closeModal();
    },
    [closeModal],
  );

  return (
    <>
      <div className="flex justify-end mb-6">
        <button
          type="button"
          onClick={openCreate}
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
            onClick={openCreate}
            className="text-sm underline underline-offset-2"
          >
            Create your first series
          </button>
        </div>
      ) : (
        <ul className="grid grid-cols-1 sm:grid-cols-2 md:grid-cols-3 lg:grid-cols-4 gap-4">
          {series.map((s) => (
            <li key={s.id}>
              <SeriesCard series={s} onEdit={() => openEdit(s.id)} />
            </li>
          ))}
        </ul>
      )}

      <SeriesEditModal
        open={target !== null}
        target={target}
        tab={tab}
        onTabChange={setTab}
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

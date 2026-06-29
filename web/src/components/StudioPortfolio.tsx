"use client";

/**
 * Portfolio dashboard — the meat of `/studio`.
 *
 *   - Status filter pills (All / Drafts / Published) — URL-driven so
 *     refreshes land back on the same view
 *   - Grid of cards with thumbnail (or "no image" placeholder), title,
 *     status badge, edit + delete buttons
 *   - "+ New artwork" button — opens the edit modal in create mode
 *
 * Edit + create both open the same `ArtworkEditModal`. The modal
 * lifecycle is URL-driven via `?id=` (matches the convention
 * established by `StudioSeriesManager`):
 *
 *   ?id=new                          → create mode
 *   ?id=<uuid>                       → edit mode for that artwork
 *   (no id)                          → modal closed
 *
 * Multi-step flow: after a create the parent does
 * `router.replace("?id=<new-uuid>")` so the artist can keep editing
 * (add image, dimensions, etc.) without a separate page transition.
 * Shareable / refresh-friendly / back-button-closes-modal.
 */

import { useCallback } from "react";
import { useRouter, useSearchParams } from "next/navigation";
import { clsx } from "clsx";
import { ArtworkEditModal } from "@/components/ArtworkEditModal";
import type {
  StudioArtist,
  StudioArtworkDetail,
  StudioArtworkSummary,
} from "@/lib/api";
import { formatMedium } from "@/lib/medium";

type StatusFilter = "all" | "draft" | "published" | "archived";

const FILTERS: Array<{ token: StatusFilter; label: string }> = [
  { token: "all", label: "All" },
  { token: "draft", label: "Drafts" },
  { token: "published", label: "Published" },
];

interface Props {
  artist: StudioArtist;
  items: StudioArtworkSummary[];
  status: StatusFilter;
}

export function StudioPortfolio({ artist, items, status }: Props) {
  const router = useRouter();
  const searchParams = useSearchParams();

  // Modal target driven by `?id=`. `"new"` = create mode; uuid = edit.
  const idParam = searchParams.get("id");
  const target: string | "new" | null = idParam ?? null;

  // Build a fresh URL preserving the current ?status filter — modal
  // lifecycle transitions shouldn't reset the artist's filter view.
  const urlWith = useCallback(
    (params: Record<string, string | null>): string => {
      const usp = new URLSearchParams(searchParams);
      for (const [k, v] of Object.entries(params)) {
        if (v == null) usp.delete(k);
        else usp.set(k, v);
      }
      const qs = usp.toString();
      return `/studio${qs ? `?${qs}` : ""}`;
    },
    [searchParams],
  );

  const openCreate = useCallback(() => {
    router.replace(urlWith({ id: "new" }), { scroll: false });
  }, [router, urlWith]);

  const openEdit = useCallback(
    (id: string) => {
      router.replace(urlWith({ id }), { scroll: false });
    },
    [router, urlWith],
  );

  const closeModal = useCallback(() => {
    router.replace(urlWith({ id: null }), { scroll: false });
    router.refresh();
  }, [router, urlWith]);

  // ArtworkEditModal calls this after every successful write
  // (create / edit / image add / image remove). `closeAfter` is
  // truthy only on edit-mode Save; otherwise we keep the modal open
  // so the artist can keep iterating. URL updates so the modal lifecycle
  // is share/refresh-friendly.
  const onSaved = useCallback(
    (detail: StudioArtworkDetail, closeAfter?: boolean) => {
      if (closeAfter) {
        closeModal();
        return;
      }
      // Falsy closeAfter happens in two cases:
      //   (a) just-finished create — URL is `?id=new`, advance to the
      //       new id so refresh / share / back-button work correctly.
      //   (b) in-modal image add/remove on an existing series — URL
      //       already reflects this id, no nav needed.
      if (target === "new") {
        router.replace(urlWith({ id: detail.id }), { scroll: false });
      }
    },
    [target, router, urlWith, closeModal],
  );

  function setStatus(next: StatusFilter) {
    router.push(urlWith({ status: next === "all" ? null : next }));
  }

  return (
    <>
      {/* Filter pills + Add button on one row. */}
      <div className="flex items-center justify-between mb-6">
        <div role="toolbar" aria-label="Filter by status" className="flex gap-2">
          {FILTERS.map((f) => (
            <button
              key={f.token}
              type="button"
              aria-pressed={status === f.token}
              onClick={() => setStatus(f.token)}
              className={clsx(
                "px-3 py-1.5 text-sm border",
                status === f.token
                  ? "border-foreground bg-foreground text-background"
                  : "border-border bg-surface hover:bg-background"
              )}
            >
              {f.label}
            </button>
          ))}
        </div>
        <button
          type="button"
          onClick={openCreate}
          className="px-4 py-2 text-sm bg-foreground text-background"
        >
          + New artwork
        </button>
      </div>

      {items.length === 0 ? (
        <EmptyState status={status} onNew={openCreate} />
      ) : (
        <ul className="grid grid-cols-1 sm:grid-cols-2 md:grid-cols-3 lg:grid-cols-4 gap-4">
          {items.map((a) => (
            <li key={a.id}>
              <ArtworkCard artwork={a} onEdit={() => openEdit(a.id)} />
            </li>
          ))}
        </ul>
      )}

      <ArtworkEditModal
        artistDisplayName={artist.display_name}
        open={target !== null}
        target={target}
        onSaved={onSaved}
        onClose={closeModal}
      />
    </>
  );
}

function ArtworkCard({
  artwork,
  onEdit,
}: {
  artwork: StudioArtworkSummary;
  onEdit: () => void;
}) {
  return (
    <article className="border border-border bg-surface">
      <div className="relative aspect-square bg-background overflow-hidden">
        {artwork.primary_image_url ? (
          // eslint-disable-next-line @next/next/no-img-element
          <img
            src={artwork.primary_image_url}
            alt={artwork.title ?? "Untitled"}
            className="absolute inset-0 w-full h-full object-cover"
          />
        ) : (
          <div className="absolute inset-0 flex items-center justify-center text-xs text-muted">
            no image
          </div>
        )}
        <StatusBadge status={artwork.status} />
      </div>
      <div className="p-3 space-y-2">
        <h3 className="font-serif text-base truncate">
          {artwork.title ?? "Untitled"}
        </h3>
        {/* T-073 — show category + materials together. Falls back to
            "—" only when both are blank (no taxonomy + no free text). */}
        <p className="text-xs text-muted">
          {formatMedium(artwork.medium_category, artwork.medium) || "—"}
        </p>
        <div className="flex gap-2 pt-2">
          <button
            type="button"
            onClick={onEdit}
            className="flex-1 px-3 py-1.5 text-xs border border-border bg-background hover:bg-surface"
          >
            Edit
          </button>
        </div>
      </div>
    </article>
  );
}

function StatusBadge({ status }: { status: StudioArtworkSummary["status"] }) {
  const label =
    status === "draft" ? "Draft" : status === "archived" ? "Archived" : "Published";
  return (
    <span
      className={clsx(
        "absolute top-2 right-2 px-2 py-0.5 text-[10px] uppercase tracking-wider",
        status === "published"
          ? "bg-foreground text-background"
          : "bg-surface border border-border"
      )}
    >
      {label}
    </span>
  );
}

function EmptyState({
  status,
  onNew,
}: {
  status: StatusFilter;
  onNew: () => void;
}) {
  if (status !== "all") {
    return (
      <p className="p-6 border border-border bg-surface text-sm text-muted">
        No {status} artworks. Switch to <em>All</em> to see everything.
      </p>
    );
  }
  return (
    <section className="p-8 border border-border bg-surface text-center">
      <h2 className="font-serif text-xl">Your portfolio is empty.</h2>
      <p className="mt-2 text-sm text-muted">
        Add your first piece to start.
      </p>
      <button
        type="button"
        onClick={onNew}
        className="mt-4 px-5 py-2 text-sm bg-foreground text-background"
      >
        + New artwork
      </button>
    </section>
  );
}

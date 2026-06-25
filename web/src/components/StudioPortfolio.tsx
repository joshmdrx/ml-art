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
 * loads details lazily when given an `id`; in create mode it starts
 * empty and creates-then-edits so the user can add an image to a
 * fresh artwork without a separate page transition.
 */

import { useState } from "react";
import { useRouter, useSearchParams } from "next/navigation";
import { clsx } from "clsx";
import { ArtworkEditModal } from "@/components/ArtworkEditModal";
import type { StudioArtist, StudioArtworkSummary } from "@/lib/api";
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

  // Modal target: `null` = closed, `"new"` = create mode, `<uuid>` = edit.
  const [open, setOpen] = useState<string | "new" | null>(null);

  function setStatus(next: StatusFilter) {
    const usp = new URLSearchParams(searchParams);
    if (next === "all") usp.delete("status");
    else usp.set("status", next);
    const qs = usp.toString();
    router.push(`/studio${qs ? `?${qs}` : ""}`);
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
          onClick={() => setOpen("new")}
          className="px-4 py-2 text-sm bg-foreground text-background"
        >
          + New artwork
        </button>
      </div>

      {items.length === 0 ? (
        <EmptyState status={status} onNew={() => setOpen("new")} />
      ) : (
        <ul className="grid grid-cols-1 sm:grid-cols-2 md:grid-cols-3 lg:grid-cols-4 gap-4">
          {items.map((a) => (
            <li key={a.id}>
              <ArtworkCard artwork={a} onEdit={() => setOpen(a.id)} />
            </li>
          ))}
        </ul>
      )}

      <ArtworkEditModal
        artistDisplayName={artist.display_name}
        open={open !== null}
        target={open}
        onClose={() => {
          setOpen(null);
          router.refresh();
        }}
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

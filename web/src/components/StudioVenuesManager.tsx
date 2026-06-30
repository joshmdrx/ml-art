"use client";

/**
 * T-081.2 — studio venues management.
 *
 * URL-driven modal lifecycle (`?id=` + `?tab=`), mirrors
 * StudioSeriesManager. See `docs/ui-patterns.md` for the rationale.
 *
 * Status badge per card so the owner sees pending_review at a glance —
 * a venue isn't public until an admin flips it via T-083's admin queue.
 */

import { useCallback, useMemo, useState } from "react";
import { useRouter, useSearchParams } from "next/navigation";
import { clsx } from "clsx";
import { VenueEditModal } from "@/components/VenueEditModal";
import type { Venue, VenueStatus } from "@/lib/api";

type Tab = "details" | "artworks";

export function StudioVenuesManager({
  initialVenues,
}: {
  initialVenues: Venue[];
}) {
  const router = useRouter();
  const searchParams = useSearchParams();
  const [venues, setVenues] = useState<Venue[]>(initialVenues);

  const idParam = searchParams.get("id"); // "new" | uuid | null
  const tabParam = searchParams.get("tab");
  const tab: Tab = tabParam === "artworks" ? "artworks" : "details";

  const target: Venue | "new" | null = useMemo(() => {
    if (idParam === "new") return "new";
    if (!idParam) return null;
    return venues.find((v) => v.id === idParam) ?? null;
  }, [idParam, venues]);

  const openCreate = useCallback(() => {
    router.replace("/studio/venues?id=new&tab=details", { scroll: false });
  }, [router]);

  const openEdit = useCallback(
    (id: string) => {
      router.replace(
        `/studio/venues?id=${encodeURIComponent(id)}&tab=details`,
        { scroll: false },
      );
    },
    [router],
  );

  const setTab = useCallback(
    (next: Tab) => {
      if (!idParam) return;
      router.replace(
        `/studio/venues?id=${encodeURIComponent(idParam)}&tab=${next}`,
        { scroll: false },
      );
    },
    [idParam, router],
  );

  const closeModal = useCallback(() => {
    router.replace("/studio/venues", { scroll: false });
    router.refresh();
  }, [router]);

  const onSaved = useCallback(
    (updated: Venue) => {
      setVenues((prev) => {
        const idx = prev.findIndex((v) => v.id === updated.id);
        if (idx === -1) return [updated, ...prev];
        const next = prev.slice();
        next[idx] = updated;
        return next;
      });
      // After create, advance to the Artworks tab so the owner can
      // invite work immediately. Edit-mode saves close.
      if (idParam === "new") {
        router.replace(
          `/studio/venues?id=${encodeURIComponent(updated.id)}&tab=artworks`,
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
      setVenues((prev) => prev.filter((v) => v.id !== id));
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
          + New venue
        </button>
      </div>

      {venues.length === 0 ? (
        <div className="border border-dashed border-border p-12 text-center">
          <p className="text-sm text-muted mb-4">
            No venues yet. List a gallery, shop, or studio collective —
            then invite artworks to be shown there.
          </p>
          <button
            type="button"
            onClick={openCreate}
            className="text-sm underline underline-offset-2"
          >
            Create your first venue
          </button>
        </div>
      ) : (
        <ul className="grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-3 gap-4">
          {venues.map((v) => (
            <li key={v.id}>
              <VenueCard venue={v} onEdit={() => openEdit(v.id)} />
            </li>
          ))}
        </ul>
      )}

      <VenueEditModal
        open={target !== null}
        target={target}
        tab={tab}
        onTabChange={setTab}
        onSaved={onSaved}
        onDeleted={onDeleted}
        onClose={closeModal}
      />
    </>
  );
}

function VenueCard({ venue, onEdit }: { venue: Venue; onEdit: () => void }) {
  return (
    <article className="border border-border bg-surface">
      <button
        type="button"
        onClick={onEdit}
        className="block w-full text-left p-4 hover:bg-background focus:outline-none focus-visible:ring-2 focus-visible:ring-foreground"
      >
        <div className="flex items-baseline justify-between gap-2">
          <h3 className="font-serif text-base line-clamp-1">{venue.name}</h3>
          <StatusBadge status={venue.status} />
        </div>
        <p className="mt-1 text-xs text-muted line-clamp-1">
          {venue.kind.replace("_", " ")}
          {venue.city ? ` · ${venue.city}` : ""}
          {venue.country ? `, ${venue.country}` : ""}
        </p>
        {venue.address && !venue.city && (
          <p className="mt-1 text-xs text-muted line-clamp-1">
            Locating… {venue.address}
          </p>
        )}
      </button>
    </article>
  );
}

function StatusBadge({ status }: { status: VenueStatus }) {
  const label =
    status === "pending_review"
      ? "Pending review"
      : status === "active"
        ? "Public"
        : status === "paused"
          ? "Paused"
          : "Declined";
  return (
    <span
      className={clsx(
        "shrink-0 text-[10px] tracking-wide uppercase px-1.5 py-0.5 border",
        status === "active"
          ? "border-foreground"
          : "border-border text-muted",
      )}
    >
      {label}
    </span>
  );
}

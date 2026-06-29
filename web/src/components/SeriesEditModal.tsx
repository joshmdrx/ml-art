"use client";

/**
 * T-058.2 — Series create/edit modal.
 *
 * Two tabs:
 *   - **Details** — title, statement, cover artwork picker.
 *   - **Artworks** — checkbox grid for membership management (the
 *     T-058 multi-select primitive).
 *
 * Create mode: starts on "Details" with empty fields; submit creates
 * the series, then flips to "Artworks" so the user can attach work
 * in the same flow.
 *
 * Edit mode: loads fresh from `target` prop (the latest `StudioSeries`
 * from the parent list), shows current membership pre-checked, lets
 * the user retitle / re-statement / re-cover / re-membership /
 * delete.
 *
 * The checkbox grid uses the artist's full artwork list (passed in by
 * the page so we don't re-fetch on open) and renders the
 * `series_id === target.id` artworks pre-checked. Save calls
 * `setSeriesArtworks` which replaces the membership atomically.
 */

import { useCallback, useEffect, useState } from "react";
import { clsx } from "clsx";
import {
  createSeries,
  deleteSeries,
  patchSeries,
  saveSeriesArtworks,
} from "@/app/actions/series";
import type { StudioArtworkSummary, StudioSeries } from "@/lib/api";
import { reportError } from "@/lib/reportError";

interface Props {
  open: boolean;
  /** `"new"` = create mode; `StudioSeries` = edit; `null` = closed. */
  target: StudioSeries | "new" | null;
  artworks: StudioArtworkSummary[];
  artistName: string;
  onSaved: (s: StudioSeries) => void;
  onDeleted: (id: string) => void;
  onClose: () => void;
}

const MAX_TITLE = 200;
const MAX_STATEMENT = 500;

type Tab = "details" | "artworks";

export function SeriesEditModal({
  open,
  target,
  artworks,
  artistName,
  onSaved,
  onDeleted,
  onClose,
}: Props) {
  // After a successful create, we self-promote into edit mode without
  // closing the modal — the parent still owns `target` and doesn't know
  // to upgrade "new" to the new id, so we shadow it with `justCreated`.
  // Lets the Artworks tab unlock + the user pick membership in the
  // same flow they used to create the series.
  const [justCreated, setJustCreated] = useState<StudioSeries | null>(null);
  const existing: StudioSeries | null =
    justCreated ?? (target && target !== "new" ? target : null);
  const isCreate = existing === null;

  const [tab, setTab] = useState<Tab>("details");
  const [title, setTitle] = useState("");
  const [statement, setStatement] = useState("");
  const [coverArtworkId, setCoverArtworkId] = useState<string | null>(null);
  const [memberIds, setMemberIds] = useState<Set<string>>(new Set());
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [confirmDelete, setConfirmDelete] = useState(false);

  // Reset state on open. (`open` toggles per modal lifecycle; target
  // shape changes between create and edit per click.) The setState
  // calls here are the load-from-external-store handshake — same
  // pattern as CalibratePanel / SaveModal. react-hooks/set-state-in-
  // effect flags them generically; here they're the intent.
  /* eslint-disable react-hooks/set-state-in-effect */
  useEffect(() => {
    if (!open) return;
    // Reset the create-mode self-promotion on each open. Without this,
    // an artist closing the modal then re-opening "+ New series" would
    // see the previously-created series's id still active.
    setJustCreated(null);
    setTab("details");
    setError(null);
    setConfirmDelete(false);
    // Compute initial form state from the parent's `target` directly
    // — NOT from the derived `existing`, which also tracks
    // `justCreated`. Keying on `existing` would re-fire this effect
    // after a successful create-mode self-promotion and immediately
    // wipe the just-created state, sending us back to a blank Details
    // tab. The parent's target only changes on open / close.
    const initial: StudioSeries | null =
      target && target !== "new" ? target : null;
    if (initial) {
      setTitle(initial.title);
      setStatement(initial.statement ?? "");
      setCoverArtworkId(initial.cover_artwork_id);
      // Pre-check artworks whose `series_id` already matches this
      // series — `StudioArtworkSummary.series_id` (T-058) surfaces it
      // server-side so the modal opens with current membership ticked.
      const members = new Set<string>();
      for (const a of artworks) {
        if (a.series_id === initial.id) {
          members.add(a.id);
        }
      }
      setMemberIds(members);
    } else {
      setTitle("");
      setStatement("");
      setCoverArtworkId(null);
      setMemberIds(new Set());
    }
  }, [open, target, artworks]);
  /* eslint-enable react-hooks/set-state-in-effect */

  const onSubmitDetails = useCallback(
    async (e: React.FormEvent) => {
      e.preventDefault();
      if (saving) return;
      const trimmed = title.trim();
      if (!trimmed) {
        setError("Title is required.");
        return;
      }
      if (trimmed.length > MAX_TITLE) {
        setError(`Title is too long (max ${MAX_TITLE} characters).`);
        return;
      }
      if (statement.length > MAX_STATEMENT) {
        setError(`Statement is too long (max ${MAX_STATEMENT} characters).`);
        return;
      }
      setSaving(true);
      setError(null);
      try {
        const body = {
          title: trimmed,
          statement: statement.trim() === "" ? null : statement.trim(),
          cover_artwork_id: coverArtworkId,
        };
        const wasCreate = isCreate;
        const saved = existing
          ? await patchSeries(existing.id, body)
          : await createSeries(body);
        onSaved(saved);
        if (wasCreate) {
          // Self-promote into edit mode for the newly-created series
          // and flip to the Artworks tab so the artist can attach
          // works in the same flow. The parent still has
          // `target === "new"`; the modal's local `justCreated` shadows
          // it so `existing`, `isCreate`, and the Artworks tab's
          // `disabled` state all flip correctly.
          setJustCreated(saved);
          setTab("artworks");
        } else {
          onClose();
        }
      } catch (e) {
        if ((e as Error).message === "conflict") {
          setError(
            "A series with that title already exists — try a different title.",
          );
        } else {
          reportError(e, { surface: "series-edit", action: "save-details" });
          setError("Save failed. Try again.");
        }
      } finally {
        setSaving(false);
      }
    },
    [
      saving,
      title,
      statement,
      coverArtworkId,
      existing,
      isCreate,
      onSaved,
      onClose,
    ],
  );

  const onSubmitArtworks = useCallback(async () => {
    if (!existing) return;
    if (saving) return;
    setSaving(true);
    setError(null);
    try {
      const ack = await saveSeriesArtworks(existing.id, Array.from(memberIds));
      // Update the parent's view of artwork_count so the card re-renders.
      onSaved({ ...existing, artwork_count: ack.artwork_count });
      onClose();
    } catch (e) {
      reportError(e, { surface: "series-edit", action: "save-artworks" });
      setError("Couldn't update membership. Try again.");
    } finally {
      setSaving(false);
    }
  }, [existing, memberIds, saving, onSaved, onClose]);

  const onDelete = useCallback(async () => {
    if (!existing) return;
    if (saving) return;
    setSaving(true);
    setError(null);
    try {
      await deleteSeries(existing.id);
      onDeleted(existing.id);
      onClose();
    } catch (e) {
      reportError(e, { surface: "series-edit", action: "delete" });
      setError("Delete failed. Try again.");
      setSaving(false);
    }
  }, [existing, saving, onDeleted, onClose]);

  if (!open) return null;

  return (
    <div
      role="dialog"
      aria-modal="true"
      aria-label={isCreate ? "Create series" : "Edit series"}
      className="fixed inset-0 z-50 bg-black/40 flex items-center justify-center p-4"
      onClick={(e) => {
        if (e.target === e.currentTarget) onClose();
      }}
    >
      <div className="bg-background border border-border w-full max-w-3xl max-h-[90vh] overflow-y-auto">
        <header className="border-b border-border p-4 flex items-baseline justify-between">
          <h2 className="font-serif text-xl">
            {isCreate ? "New series" : existing?.title ?? "Series"}
          </h2>
          <button
            type="button"
            onClick={onClose}
            className="text-sm text-muted hover:text-foreground"
            aria-label="Close"
          >
            ✕
          </button>
        </header>

        {/* Tabs — Artworks tab is disabled in create mode until the
            series has an id to assign membership against. */}
        <nav role="tablist" className="border-b border-border flex">
          <TabButton
            active={tab === "details"}
            onClick={() => setTab("details")}
          >
            Details
          </TabButton>
          <TabButton
            active={tab === "artworks"}
            disabled={isCreate}
            onClick={() => setTab("artworks")}
          >
            Artworks
            {existing ? ` (${existing.artwork_count})` : ""}
          </TabButton>
        </nav>

        {error && (
          <div className="p-3 mx-4 mt-4 border border-foreground bg-surface text-sm">
            {error}
          </div>
        )}

        {tab === "details" ? (
          <DetailsTab
            title={title}
            setTitle={setTitle}
            statement={statement}
            setStatement={setStatement}
            coverArtworkId={coverArtworkId}
            setCoverArtworkId={setCoverArtworkId}
            artworks={artworks}
            artistName={artistName}
            saving={saving}
            isCreate={isCreate}
            confirmDelete={confirmDelete}
            setConfirmDelete={setConfirmDelete}
            onSubmit={onSubmitDetails}
            onDelete={existing ? onDelete : null}
            onClose={onClose}
          />
        ) : (
          <ArtworksTab
            artworks={artworks}
            memberIds={memberIds}
            setMemberIds={setMemberIds}
            saving={saving}
            onSubmit={onSubmitArtworks}
            onClose={onClose}
          />
        )}
      </div>
    </div>
  );
}

function TabButton({
  active,
  disabled,
  onClick,
  children,
}: {
  active: boolean;
  disabled?: boolean;
  onClick: () => void;
  children: React.ReactNode;
}) {
  return (
    <button
      type="button"
      role="tab"
      aria-selected={active}
      disabled={disabled}
      onClick={onClick}
      className={clsx(
        "px-4 py-3 text-sm border-b-2",
        active
          ? "border-foreground"
          : "border-transparent text-muted hover:text-foreground",
        disabled && "opacity-40 cursor-not-allowed hover:text-muted",
      )}
    >
      {children}
    </button>
  );
}

// ─────────────────────────────────────────────────────────────────────────────
// Details tab — title, statement, cover picker
// ─────────────────────────────────────────────────────────────────────────────

function DetailsTab({
  title,
  setTitle,
  statement,
  setStatement,
  coverArtworkId,
  setCoverArtworkId,
  artworks,
  saving,
  isCreate,
  confirmDelete,
  setConfirmDelete,
  onSubmit,
  onDelete,
  onClose,
}: {
  title: string;
  setTitle: (v: string) => void;
  statement: string;
  setStatement: (v: string) => void;
  coverArtworkId: string | null;
  setCoverArtworkId: (v: string | null) => void;
  artworks: StudioArtworkSummary[];
  artistName: string;
  saving: boolean;
  isCreate: boolean;
  confirmDelete: boolean;
  setConfirmDelete: (v: boolean) => void;
  onSubmit: (e: React.FormEvent) => void;
  onDelete: (() => void) | null;
  onClose: () => void;
}) {
  return (
    <form onSubmit={onSubmit} className="p-4 space-y-4">
      <div>
        <label htmlFor="series-title" className="block text-sm mb-1">
          Title
        </label>
        <input
          id="series-title"
          type="text"
          required
          maxLength={MAX_TITLE}
          value={title}
          onChange={(e) => setTitle(e.target.value)}
          className="w-full border border-border bg-surface px-3 py-2"
          placeholder="e.g. Quiet Mornings"
        />
      </div>

      <div>
        <label htmlFor="series-statement" className="block text-sm mb-1">
          Statement{" "}
          <span className="text-xs text-muted">(optional, max {MAX_STATEMENT})</span>
        </label>
        <textarea
          id="series-statement"
          value={statement}
          onChange={(e) => setStatement(e.target.value)}
          maxLength={MAX_STATEMENT}
          rows={3}
          className="w-full border border-border bg-surface px-3 py-2"
          placeholder="What ties this work together?"
        />
        <p className="text-xs text-muted mt-1">
          {statement.length} / {MAX_STATEMENT}
        </p>
      </div>

      <div>
        <label htmlFor="series-cover" className="block text-sm mb-1">
          Cover image{" "}
          <span className="text-xs text-muted">
            (defaults to first member if not picked)
          </span>
        </label>
        <select
          id="series-cover"
          value={coverArtworkId ?? ""}
          onChange={(e) =>
            setCoverArtworkId(e.target.value === "" ? null : e.target.value)
          }
          className="w-full border border-border bg-surface px-3 py-2"
        >
          <option value="">— None (auto) —</option>
          {artworks.map((a) => (
            <option key={a.id} value={a.id}>
              {a.title ?? "Untitled"}
            </option>
          ))}
        </select>
      </div>

      <div className="flex items-center justify-between pt-4 border-t border-border">
        <div>
          {onDelete && !confirmDelete && (
            <button
              type="button"
              onClick={() => setConfirmDelete(true)}
              disabled={saving}
              className="text-sm text-muted hover:text-foreground"
            >
              Delete series
            </button>
          )}
          {onDelete && confirmDelete && (
            <div className="flex items-center gap-2 text-sm">
              <span>Delete this series?</span>
              <button
                type="button"
                onClick={onDelete}
                disabled={saving}
                className="px-2 py-1 border border-foreground bg-foreground text-background"
              >
                Yes, delete
              </button>
              <button
                type="button"
                onClick={() => setConfirmDelete(false)}
                disabled={saving}
                className="text-muted hover:text-foreground"
              >
                Cancel
              </button>
            </div>
          )}
        </div>
        <div className="flex items-center gap-3">
          <button
            type="button"
            onClick={onClose}
            disabled={saving}
            className="text-sm text-muted hover:text-foreground"
          >
            Cancel
          </button>
          <button
            type="submit"
            disabled={saving || title.trim() === ""}
            className="px-4 py-2 text-sm bg-foreground text-background disabled:opacity-50"
          >
            {saving ? "Saving…" : isCreate ? "Create" : "Save"}
          </button>
        </div>
      </div>
    </form>
  );
}

// ─────────────────────────────────────────────────────────────────────────────
// Artworks tab — checkbox grid for membership
// ─────────────────────────────────────────────────────────────────────────────

function ArtworksTab({
  artworks,
  memberIds,
  setMemberIds,
  saving,
  onSubmit,
  onClose,
}: {
  artworks: StudioArtworkSummary[];
  memberIds: Set<string>;
  setMemberIds: (v: Set<string>) => void;
  saving: boolean;
  onSubmit: () => void;
  onClose: () => void;
}) {
  const toggle = (id: string) => {
    const next = new Set(memberIds);
    if (next.has(id)) {
      next.delete(id);
    } else {
      next.add(id);
    }
    setMemberIds(next);
  };

  return (
    <div className="p-4 space-y-4">
      <p className="text-sm text-muted">
        Tick the works that belong in this series. Click Save to apply —
        unchecked works currently in the series will be removed.
      </p>

      {artworks.length === 0 ? (
        <div className="border border-dashed border-border p-6 text-center text-sm text-muted">
          You don&apos;t have any artworks yet. Add a few from the portfolio,
          then come back here.
        </div>
      ) : (
        <ul className="grid grid-cols-2 sm:grid-cols-3 md:grid-cols-4 gap-3 max-h-[50vh] overflow-y-auto">
          {artworks.map((a) => {
            const checked = memberIds.has(a.id);
            return (
              <li key={a.id}>
                <label
                  className={clsx(
                    "block border cursor-pointer overflow-hidden",
                    checked
                      ? "border-foreground ring-2 ring-foreground"
                      : "border-border hover:border-foreground",
                  )}
                >
                  <input
                    type="checkbox"
                    checked={checked}
                    onChange={() => toggle(a.id)}
                    className="sr-only"
                  />
                  <div className="relative aspect-square bg-background">
                    {a.primary_image_url ? (
                      // eslint-disable-next-line @next/next/no-img-element
                      <img
                        src={a.primary_image_url}
                        alt={a.title ?? "Untitled"}
                        loading="lazy"
                        className="w-full h-full object-cover"
                      />
                    ) : (
                      <div className="w-full h-full flex items-center justify-center text-xs text-muted">
                        No image
                      </div>
                    )}
                    {checked && (
                      <span className="absolute top-1 right-1 bg-foreground text-background w-5 h-5 flex items-center justify-center text-xs">
                        ✓
                      </span>
                    )}
                  </div>
                  <div className="p-2 text-xs">
                    <div className="line-clamp-1">{a.title ?? "Untitled"}</div>
                  </div>
                </label>
              </li>
            );
          })}
        </ul>
      )}

      <div className="flex items-center justify-between pt-4 border-t border-border">
        <p className="text-xs text-muted">
          {memberIds.size}{" "}
          {memberIds.size === 1 ? "work selected" : "works selected"}
        </p>
        <div className="flex items-center gap-3">
          <button
            type="button"
            onClick={onClose}
            disabled={saving}
            className="text-sm text-muted hover:text-foreground"
          >
            Cancel
          </button>
          <button
            type="button"
            onClick={onSubmit}
            disabled={saving}
            className="px-4 py-2 text-sm bg-foreground text-background disabled:opacity-50"
          >
            {saving ? "Saving…" : "Save"}
          </button>
        </div>
      </div>
    </div>
  );
}

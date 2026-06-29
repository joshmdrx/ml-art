"use client";

/**
 * T-058.2 — Series create/edit modal.
 *
 * Two tabs:
 *   - **Details** — title, statement, cover artwork picker.
 *   - **Artworks** — checkbox grid for membership management (the
 *     T-058 multi-select primitive).
 *
 * Lifecycle (see `docs/ui-patterns.md` → Modal behaviour):
 *   - Modal lifecycle (open/closed, current tab, current series) is
 *     driven by URL params owned by the parent. The modal is a dumb
 *     renderer of (target, tab) — it doesn't manage its own
 *     visibility or tab state.
 *   - Modal calls `onSaved(s)` after a successful save; parent decides
 *     what URL state comes next (close, or advance to the new series
 *     on the Artworks tab for the create-then-attach flow).
 *   - Tab clicks call `onTabChange(next)`; parent updates the URL.
 *
 * Feedback (per ui-patterns):
 *   - `toast.success(...)` on create, save, delete.
 *   - Inline alert banner for form-level errors (network / 500).
 *   - `<FieldError>` for field-level validation (title required, etc.).
 *   - `useConfirm()` for the destructive delete confirmation.
 */

import { useCallback, useEffect, useState } from "react";
import { clsx } from "clsx";
import { toast } from "sonner";
import {
  createSeries,
  deleteSeries,
  patchSeries,
  saveSeriesArtworks,
} from "@/app/actions/series";
import { FieldError } from "@/components/ui/FieldError";
import { useConfirm } from "@/components/ui/ConfirmDialog";
import type { StudioArtworkSummary, StudioSeries } from "@/lib/api";
import { reportError } from "@/lib/reportError";

type Tab = "details" | "artworks";

interface Props {
  open: boolean;
  /** `"new"` = create mode; `StudioSeries` = edit; `null` = closed. */
  target: StudioSeries | "new" | null;
  /** Current tab, owned by the parent (URL-driven). */
  tab: Tab;
  /** Called when the user clicks a tab. Parent updates the URL. */
  onTabChange: (next: Tab) => void;
  artworks: StudioArtworkSummary[];
  artistName: string;
  onSaved: (s: StudioSeries) => void;
  onDeleted: (id: string) => void;
  onClose: () => void;
}

const MAX_TITLE = 200;
const MAX_STATEMENT = 500;

export function SeriesEditModal({
  open,
  target,
  tab,
  onTabChange,
  artworks,
  onSaved,
  onDeleted,
  onClose,
}: Props) {
  const existing: StudioSeries | null =
    target && target !== "new" ? target : null;
  const isCreate = existing === null;

  const confirm = useConfirm();

  const [title, setTitle] = useState("");
  const [titleError, setTitleError] = useState<string | null>(null);
  const [statement, setStatement] = useState("");
  const [coverArtworkId, setCoverArtworkId] = useState<string | null>(null);
  const [memberIds, setMemberIds] = useState<Set<string>>(new Set());
  const [saving, setSaving] = useState(false);
  const [formError, setFormError] = useState<string | null>(null);

  // Reset form state whenever the modal opens for a different target.
  // Tab is owned by the parent via URL params; we don't manage it
  // locally anymore. Membership pre-check uses
  // `StudioArtworkSummary.series_id` (T-058) so the grid opens with
  // current members ticked without a second round-trip.
  //
  // The setState calls here are the load-from-external-store handshake
  // — same shape SaveModal / CalibratePanel use. The
  // react-hooks/set-state-in-effect rule flags them generically; in
  // this load-on-open pattern they're the intent.
  /* eslint-disable react-hooks/set-state-in-effect */
  useEffect(() => {
    if (!open) return;
    setFormError(null);
    setTitleError(null);
    if (existing) {
      setTitle(existing.title);
      setStatement(existing.statement ?? "");
      setCoverArtworkId(existing.cover_artwork_id);
      const members = new Set<string>();
      for (const a of artworks) {
        if (a.series_id === existing.id) {
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
  }, [open, existing, artworks]);
  /* eslint-enable react-hooks/set-state-in-effect */

  const validateTitle = useCallback((raw: string): string | null => {
    const trimmed = raw.trim();
    if (!trimmed) return "Title is required.";
    if (trimmed.length > MAX_TITLE) {
      return `Title is too long (max ${MAX_TITLE} characters).`;
    }
    return null;
  }, []);

  const onSubmitDetails = useCallback(
    async (e: React.FormEvent) => {
      e.preventDefault();
      if (saving) return;

      const titleErr = validateTitle(title);
      setTitleError(titleErr);
      if (titleErr) return;
      if (statement.length > MAX_STATEMENT) {
        setFormError(
          `Statement is too long (max ${MAX_STATEMENT} characters).`,
        );
        return;
      }

      setSaving(true);
      setFormError(null);
      try {
        const body = {
          title: title.trim(),
          statement: statement.trim() === "" ? null : statement.trim(),
          cover_artwork_id: coverArtworkId,
        };
        const saved = existing
          ? await patchSeries(existing.id, body)
          : await createSeries(body);
        toast.success(isCreate ? "Series created" : "Saved");
        // Parent decides what happens next. Create → re-opens with the
        // new series as target (flips us into edit mode + Artworks tab
        // via the prevTargetRef detection above). Edit → closes.
        onSaved(saved);
      } catch (err) {
        if ((err as Error).message === "conflict") {
          setFormError(
            "A series with that title already exists — try a different title.",
          );
        } else {
          reportError(err, { surface: "series-edit", action: "save-details" });
          setFormError("Save failed. Try again.");
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
      validateTitle,
      onSaved,
    ],
  );

  const onSubmitArtworks = useCallback(async () => {
    if (!existing || saving) return;
    setSaving(true);
    setFormError(null);
    try {
      const ack = await saveSeriesArtworks(existing.id, Array.from(memberIds));
      toast.success(
        ack.artwork_count === 1
          ? "1 work in series"
          : `${ack.artwork_count} works in series`,
      );
      onSaved({ ...existing, artwork_count: ack.artwork_count });
    } catch (err) {
      reportError(err, { surface: "series-edit", action: "save-artworks" });
      setFormError("Couldn't update membership. Try again.");
    } finally {
      setSaving(false);
    }
  }, [existing, memberIds, saving, onSaved]);

  const onDelete = useCallback(async () => {
    if (!existing || saving) return;
    const proceed = await confirm({
      title: `Delete “${existing.title}”?`,
      description: "This can't be undone. Member artworks are kept; they just stop belonging to a series.",
      confirmLabel: "Delete",
      destructive: true,
    });
    if (!proceed) return;
    setSaving(true);
    setFormError(null);
    try {
      await deleteSeries(existing.id);
      toast.success("Series deleted");
      onDeleted(existing.id);
    } catch (err) {
      reportError(err, { surface: "series-edit", action: "delete" });
      setFormError("Delete failed. Try again.");
      setSaving(false);
    }
  }, [existing, saving, confirm, onDeleted]);

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

        {/* Tabs — Artworks tab is disabled in create mode (we don't
            have an id to assign membership against yet). Parent
            re-opens with the new series after create; useRef flips us
            to this tab automatically. */}
        <nav role="tablist" className="border-b border-border flex">
          <TabButton
            active={tab === "details"}
            onClick={() => onTabChange("details")}
          >
            Details
          </TabButton>
          <TabButton
            active={tab === "artworks"}
            disabled={isCreate}
            onClick={() => onTabChange("artworks")}
          >
            Artworks
            {existing ? ` (${existing.artwork_count})` : ""}
          </TabButton>
        </nav>

        {formError && (
          <div
            role="alert"
            className="p-3 mx-4 mt-4 border border-foreground bg-surface text-sm"
          >
            {formError}
          </div>
        )}

        {tab === "details" ? (
          <DetailsTab
            title={title}
            setTitle={(v) => {
              setTitle(v);
              if (titleError) setTitleError(validateTitle(v));
            }}
            titleError={titleError}
            statement={statement}
            setStatement={setStatement}
            coverArtworkId={coverArtworkId}
            setCoverArtworkId={setCoverArtworkId}
            artworks={artworks}
            saving={saving}
            isCreate={isCreate}
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
  titleError,
  statement,
  setStatement,
  coverArtworkId,
  setCoverArtworkId,
  artworks,
  saving,
  isCreate,
  onSubmit,
  onDelete,
  onClose,
}: {
  title: string;
  setTitle: (v: string) => void;
  titleError: string | null;
  statement: string;
  setStatement: (v: string) => void;
  coverArtworkId: string | null;
  setCoverArtworkId: (v: string | null) => void;
  artworks: StudioArtworkSummary[];
  saving: boolean;
  isCreate: boolean;
  onSubmit: (e: React.FormEvent) => void;
  onDelete: (() => void) | null;
  onClose: () => void;
}) {
  return (
    <form onSubmit={onSubmit} className="p-4 space-y-4" noValidate>
      <div>
        <label htmlFor="series-title" className="block text-sm mb-1">
          Title
        </label>
        <input
          id="series-title"
          type="text"
          value={title}
          onChange={(e) => setTitle(e.target.value)}
          className="w-full border border-border bg-surface px-3 py-2"
          placeholder="e.g. Quiet Mornings"
          aria-invalid={titleError != null}
          aria-describedby={titleError ? "series-title-error" : undefined}
        />
        <FieldError message={titleError} />
      </div>

      <div>
        <label htmlFor="series-statement" className="block text-sm mb-1">
          Statement{" "}
          <span className="text-xs text-muted">
            (optional, max {MAX_STATEMENT})
          </span>
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
          {onDelete && (
            <button
              type="button"
              onClick={onDelete}
              disabled={saving}
              className="text-sm text-muted hover:text-foreground"
            >
              Delete series
            </button>
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
            disabled={saving}
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

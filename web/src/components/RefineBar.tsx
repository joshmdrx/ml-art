"use client";

/**
 * T-082 — "Refine with text" bar on /search.
 *
 * Pairs a free-form text input with the existing search anchors (text q,
 * image upload, seed artwork). The backend treats this as a fourth RRF
 * channel; here it's a small affordance that expands into an input on
 * click. URL-driven via `?refine=…`, same pattern as the rest of /search.
 *
 * Only renders when a primary signal is present (q OR image_upload_id OR
 * seed_artwork_id) — alone, refine would be a regular text search and
 * the backend silently drops it. Mirror that gate on the UI side so the
 * affordance never lies.
 *
 * Layout sits between ModifierBar and FilterBar: visually parallel to
 * Modify (one-click δ-vector toggles), but the input is free-form. The
 * two coexist — refine adds a separate signal channel, doesn't shift the
 * anchor vector.
 */

import { useRouter, useSearchParams } from "next/navigation";
import { useEffect, useRef, useState, useTransition } from "react";
import { clsx } from "clsx";

const REFINE_MAX_LEN = 500;

export function RefineBar() {
  const router = useRouter();
  const searchParams = useSearchParams();
  const [isPending, startTransition] = useTransition();
  const [editing, setEditing] = useState(false);
  const inputRef = useRef<HTMLInputElement>(null);

  const current = (searchParams.get("refine") ?? "").trim();
  // Draft is only meaningful in edit mode — initialised at edit-open
  // (see the buttons below) rather than synced from `current` via an
  // effect. Avoids a cascading set-state pattern + the URL is the
  // single source of truth.
  const [draft, setDraft] = useState("");

  // Auto-focus the input when expanding into edit mode.
  useEffect(() => {
    if (editing) inputRef.current?.focus();
  }, [editing]);

  // Gate the bar on the primary-signal presence; mirror the backend.
  const hasText = Boolean(searchParams.get("q")?.trim());
  const hasImage = Boolean(searchParams.get("image_upload_id"));
  const hasSeed = Boolean(searchParams.get("seed_artwork_id"));
  if (!hasText && !hasImage && !hasSeed) return null;

  function commit(next: string) {
    const value = next.trim().slice(0, REFINE_MAX_LEN);
    const usp = new URLSearchParams(searchParams);
    if (value === "") usp.delete("refine");
    else usp.set("refine", value);
    const qs = usp.toString();
    startTransition(() => router.push(`/search${qs ? `?${qs}` : ""}`));
  }

  return (
    <div
      role="toolbar"
      aria-label="Refine with text"
      className={clsx(
        "mb-6 flex flex-wrap items-center gap-2",
        isPending && "opacity-60",
      )}
    >
      <span className="text-xs text-muted mr-1">Refine:</span>
      {editing ? (
        <form
          onSubmit={(e) => {
            e.preventDefault();
            commit(draft);
            setEditing(false);
          }}
          className="inline-flex items-center gap-1"
        >
          <input
            ref={inputRef}
            type="text"
            value={draft}
            onChange={(e) => setDraft(e.target.value.slice(0, REFINE_MAX_LEN))}
            onBlur={() => {
              // Commit on blur so clicking away applies the change.
              if (draft.trim() !== current) commit(draft);
              setEditing(false);
            }}
            onKeyDown={(e) => {
              if (e.key === "Escape") {
                setDraft(current);
                setEditing(false);
              }
            }}
            placeholder="e.g. more abstract, warmer palette"
            aria-label="Refine text"
            maxLength={REFINE_MAX_LEN}
            className="px-3 py-1.5 text-sm border border-foreground bg-surface focus:outline-none w-72"
          />
          <button
            type="submit"
            disabled={isPending}
            className="px-3 py-1.5 text-sm border border-foreground bg-foreground text-background"
          >
            Apply
          </button>
        </form>
      ) : current ? (
        <div className="inline-flex items-center border text-sm border-foreground bg-foreground text-background">
          <button
            type="button"
            onClick={() => {
              setDraft(current);
              setEditing(true);
            }}
            className="px-3 py-1.5"
          >
            {truncate(current, 60)}
          </button>
          <button
            type="button"
            aria-label="Clear refine"
            onClick={() => commit("")}
            className="pr-3 pl-1 py-1.5 hover:opacity-80"
          >
            ×
          </button>
        </div>
      ) : (
        <button
          type="button"
          onClick={() => {
            setDraft("");
            setEditing(true);
          }}
          className="px-3 py-1.5 text-sm border border-border bg-surface hover:bg-background"
        >
          + Add refinement
        </button>
      )}
    </div>
  );
}

function truncate(s: string, n: number): string {
  return s.length > n ? `${s.slice(0, n - 1)}…` : s;
}

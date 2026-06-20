"use client";

/**
 * Save-to-collection modal. Renders inside the artwork detail page; opens
 * when `<SaveButton>` is clicked.
 *
 * The list of collections is fetched on first open (server action), not
 * passed eagerly as props — keeps the artwork page render lighter and
 * means the list is always fresh.
 */

import * as Dialog from "@radix-ui/react-dialog";
import { useEffect, useState, useTransition, type FormEvent } from "react";
import { clsx } from "clsx";
import type { CollectionSummary } from "@/lib/api";
import { toUserMessage } from "@/lib/reportError";
import {
  createCollectionWithFirstArtwork,
  fetchMyCollectionsForArtwork,
  saveArtworkToCollection,
  unsaveArtworkFromCollection,
} from "@/app/actions/collections";

type State =
  | { kind: "idle" }
  | { kind: "loading" }
  | { kind: "ready"; collections: CollectionSummary[]; saved: Set<string> }
  | { kind: "error"; message: string };

export function SaveModal({
  open,
  onOpenChange,
  artworkId,
}: {
  open: boolean;
  onOpenChange: (next: boolean) => void;
  artworkId: string;
}) {
  const [state, setState] = useState<State>({ kind: "idle" });
  const [newName, setNewName] = useState("");
  const [isPending, startTransition] = useTransition();

  // Reset is driven by the close action, not by `useEffect`-on-`!open`,
  // because react-hooks/set-state-in-effect rightly flags synchronous
  // setState inside an effect body. The reset is a user-action consequence,
  // not a sync to an external system.
  function handleOpenChange(next: boolean) {
    if (!next) {
      setState({ kind: "idle" });
      setNewName("");
    }
    onOpenChange(next);
  }

  // Fetch collections each time the modal opens. Uses a `cancelled` flag
  // so a fast close→open re-issue doesn't write stale data into state.
  useEffect(() => {
    if (!open) return;
    let cancelled = false;
    // Transition idle → loading at the start of a fetch. This is the
    // "external system sync" use case `useEffect` is for; the lint's
    // general guidance against synchronous setState doesn't apply when
    // the setState IS the state-machine transition we want on open.
    // eslint-disable-next-line react-hooks/set-state-in-effect
    setState({ kind: "loading" });
    fetchMyCollectionsForArtwork(artworkId)
      .then(({ collections, saved }) => {
        if (!cancelled) setState({ kind: "ready", collections, saved });
      })
      .catch((e) => {
        if (!cancelled) {
          setState({
            kind: "error",
            message: toUserMessage(
              e,
              "Couldn't load your collections. Try again in a moment.",
              { surface: "save-modal", call: "list", artworkId },
            ),
          });
        }
      });
    return () => {
      cancelled = true;
    };
  }, [open, artworkId]);

  function toggle(collection: CollectionSummary) {
    if (state.kind !== "ready") return;
    const isSaved = state.saved.has(collection.id);
    // Optimistic update
    const next = new Set(state.saved);
    if (isSaved) next.delete(collection.id);
    else next.add(collection.id);
    setState({ ...state, saved: next });

    startTransition(async () => {
      try {
        if (isSaved) {
          await unsaveArtworkFromCollection(collection.id, artworkId);
        } else {
          await saveArtworkToCollection(collection.id, artworkId);
        }
      } catch (e) {
        // Revert on failure
        setState((s) =>
          s.kind === "ready"
            ? {
                kind: "ready",
                collections: s.collections,
                saved: new Set(state.saved),
              }
            : s
        );
        setState({
          kind: "error",
          message: toUserMessage(
            e,
            isSaved
              ? "Couldn't remove from this collection. Try again."
              : "Couldn't save to this collection. Try again.",
            { surface: "save-modal", call: "toggle", artworkId },
          ),
        });
      }
    });
  }

  function onCreate(e: FormEvent<HTMLFormElement>) {
    e.preventDefault();
    const trimmed = newName.trim();
    if (!trimmed || state.kind !== "ready") return;
    startTransition(async () => {
      try {
        const created = await createCollectionWithFirstArtwork(
          trimmed,
          artworkId
        );
        setState({
          kind: "ready",
          collections: [created, ...state.collections],
          saved: new Set([...state.saved, created.id]),
        });
        setNewName("");
      } catch (e) {
        setState({
          kind: "error",
          message: toUserMessage(
            e,
            "Couldn't create that collection. Try again.",
            { surface: "save-modal", call: "create", artworkId },
          ),
        });
      }
    });
  }

  return (
    <Dialog.Root open={open} onOpenChange={handleOpenChange}>
      <Dialog.Portal>
        <Dialog.Overlay className="fixed inset-0 bg-foreground/30 backdrop-blur-sm z-40" />
        <Dialog.Content
          className="fixed left-1/2 top-1/2 -translate-x-1/2 -translate-y-1/2 z-50 w-[92vw] max-w-md bg-surface border border-border p-6 shadow-2xl"
          aria-describedby={undefined}
        >
          <Dialog.Title className="font-serif text-2xl mb-1">
            Save to collection
          </Dialog.Title>
          <Dialog.Description className="sr-only">
            Pick an existing collection or create a new one.
          </Dialog.Description>

          {state.kind === "loading" && (
            <p className="text-sm text-muted py-6">Loading…</p>
          )}

          {state.kind === "error" && (
            <p className="text-sm text-foreground py-4">{state.message}</p>
          )}

          {state.kind === "ready" && (
            <>
              {state.collections.length > 0 ? (
                <ul className="mt-4 max-h-64 overflow-y-auto -mx-2">
                  {state.collections.map((c) => {
                    const isSaved = state.saved.has(c.id);
                    return (
                      <li key={c.id}>
                        <button
                          type="button"
                          onClick={() => toggle(c)}
                          disabled={isPending}
                          aria-pressed={isSaved}
                          className={clsx(
                            "w-full text-left px-2 py-2 flex items-center gap-3 text-sm",
                            "hover:bg-background transition-colors"
                          )}
                        >
                          <span
                            className={clsx(
                              "inline-block w-4 h-4 border border-border shrink-0",
                              isSaved && "bg-foreground"
                            )}
                            aria-hidden
                          />
                          <span className="flex-1 truncate">{c.name}</span>
                          <span className="text-xs text-muted">
                            {c.artwork_count}
                          </span>
                        </button>
                      </li>
                    );
                  })}
                </ul>
              ) : (
                <p className="text-sm text-muted py-4">
                  No collections yet — make your first below.
                </p>
              )}

              <form onSubmit={onCreate} className="mt-4 flex gap-2">
                <input
                  type="text"
                  value={newName}
                  onChange={(e) => setNewName(e.target.value)}
                  placeholder="+ New collection"
                  maxLength={80}
                  aria-label="New collection name"
                  className="flex-1 bg-background border border-border px-3 py-2 text-sm focus:outline-none focus:border-foreground"
                />
                <button
                  type="submit"
                  disabled={!newName.trim() || isPending}
                  className="px-4 py-2 text-sm bg-foreground text-background disabled:opacity-40"
                >
                  Create
                </button>
              </form>
            </>
          )}

          <Dialog.Close asChild>
            <button
              type="button"
              aria-label="Close"
              className="absolute top-3 right-3 text-muted hover:text-foreground"
            >
              ×
            </button>
          </Dialog.Close>
        </Dialog.Content>
      </Dialog.Portal>
    </Dialog.Root>
  );
}

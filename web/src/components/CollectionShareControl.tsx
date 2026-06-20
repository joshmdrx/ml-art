"use client";

import { useState, useTransition } from "react";
import { useRouter } from "next/navigation";
import type { CollectionSummary } from "@/lib/api";
import { toUserMessage } from "@/lib/reportError";
import { setCollectionPublicState } from "@/app/actions/collections";

/**
 * T-053 — owner-side control for toggling a collection public/private
 * and copying the public link.
 *
 * Server passes the current `is_public` + `share_id` state; the
 * component mirrors that in local state for optimistic UI, then calls
 * `patchCollection` to flip and re-renders the parent route to refresh
 * the data.
 */
export function CollectionShareControl({
  collectionId,
  initial,
}: {
  collectionId: string;
  initial: Pick<CollectionSummary, "is_public" | "share_id">;
}) {
  const router = useRouter();
  const [isPublic, setIsPublic] = useState(initial.is_public);
  const [shareId, setShareId] = useState(initial.share_id);
  const [copied, setCopied] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [pending, startTransition] = useTransition();

  const shareUrl =
    isPublic && shareId
      ? `${typeof window !== "undefined" ? window.location.origin : "https://wander.gallery"}/c/${shareId}`
      : null;

  function toggle(next: boolean) {
    setError(null);
    startTransition(async () => {
      try {
        const updated = await setCollectionPublicState(collectionId, next);
        setIsPublic(updated.is_public);
        setShareId(updated.share_id);
        // Refresh the server component so the "Public" badge in the
        // header and any other derived state stay consistent.
        router.refresh();
      } catch (e) {
        setError(
          toUserMessage(e, "Couldn't update sharing. Try again.", {
            surface: "collection-share-control",
            collectionId,
          }),
        );
      }
    });
  }

  async function copy() {
    if (!shareUrl) return;
    try {
      await navigator.clipboard.writeText(shareUrl);
      setCopied(true);
      setTimeout(() => setCopied(false), 1800);
    } catch {
      // Clipboard can fail on insecure contexts; just leave the URL
      // visible for manual copy.
    }
  }

  return (
    <section className="mt-8 border-t border-border pt-6 max-w-3xl">
      <h2 className="font-serif text-xl mb-3">Sharing</h2>

      {!isPublic ? (
        <div className="flex flex-col gap-2 text-sm">
          <p className="text-muted">
            This collection is private. Make it public to share it via a
            link — anyone with the link can view, no sign-in required.
          </p>
          <div>
            <button
              type="button"
              onClick={() => toggle(true)}
              disabled={pending}
              className="inline-flex items-center px-4 py-2 bg-foreground text-background hover:bg-foreground/90 transition-colors disabled:opacity-60"
            >
              {pending ? "Making public…" : "Make public"}
            </button>
          </div>
        </div>
      ) : (
        <div className="flex flex-col gap-3 text-sm">
          <p className="text-muted">
            This collection is public. Anyone with this link can view it.
          </p>
          {shareUrl && (
            <div className="flex items-stretch gap-2">
              <input
                type="text"
                readOnly
                value={shareUrl}
                onFocus={(e) => e.currentTarget.select()}
                className="flex-1 bg-surface border border-border px-3 py-2 font-mono text-xs"
              />
              <button
                type="button"
                onClick={copy}
                className="px-3 py-2 border border-border bg-surface hover:bg-background transition-colors text-xs"
              >
                {copied ? "Copied" : "Copy"}
              </button>
            </div>
          )}
          <div>
            <button
              type="button"
              onClick={() => toggle(false)}
              disabled={pending}
              className="text-xs text-muted hover:text-foreground underline disabled:opacity-60"
            >
              {pending ? "Making private…" : "Make private"}
            </button>
            <span className="text-xs text-muted ml-3">
              Making it private rotates the link — the current one will stop
              working even if you re-share later.
            </span>
          </div>
        </div>
      )}

      {error && (
        <p className="mt-2 text-xs text-foreground">{error}</p>
      )}
    </section>
  );
}

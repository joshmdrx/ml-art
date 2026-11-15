"use client";

/**
 * T-086 — one draft on `/admin/drafts`.
 *
 * Unclaimed drafts get: Edit in studio, Create/replace private link,
 * Take down. Claimed ones are the organisation's now, so the row is
 * read-only apart from a link to their page.
 *
 * A freshly minted link is shown once, inline, with a copy button —
 * only its hash is stored, so it can't be shown again later (mint a new
 * one instead, which revokes the old). Replacing a live link and taking
 * a draft down both go through `useConfirm`.
 */

import { useState, useTransition } from "react";
import Link from "next/link";
import { useRouter } from "next/navigation";
import { toast } from "sonner";
import { useConfirm } from "@/components/ui/ConfirmDialog";
import {
  createPreviewLinkAction,
  startEditingDraftAction,
  takedownDraftAction,
} from "@/app/actions/admin";
import { toUserMessage } from "@/lib/reportError";
import type { AdminDraftItem } from "@/lib/api";

const BTN =
  "px-3 py-1.5 text-sm border border-border bg-surface hover:bg-background disabled:opacity-40";
const BTN_PRIMARY =
  "px-3 py-1.5 text-sm border border-foreground bg-foreground text-background disabled:opacity-40";

function formatDate(iso: string): string {
  return new Date(iso).toLocaleDateString("en-GB", {
    day: "numeric",
    month: "short",
    year: "numeric",
  });
}

export function DraftRow({ draft }: { draft: AdminDraftItem }) {
  const router = useRouter();
  const confirm = useConfirm();
  const [isPending, startTransition] = useTransition();
  const [link, setLink] = useState<{ url: string; expires_at: string } | null>(null);

  function editInStudio() {
    startTransition(async () => {
      try {
        await startEditingDraftAction(draft.id);
        router.push("/studio");
      } catch (e) {
        toast.error(
          toUserMessage(e, `Couldn't open ${draft.display_name} in the studio.`, {
            surface: "admin-drafts",
            action: "edit",
          }),
        );
      }
    });
  }

  async function mintLink() {
    if (draft.live_link_expires_at) {
      const ok = await confirm({
        title: "Replace the current link?",
        description:
          "The link you already sent will stop working. Only the new one will open this draft.",
        confirmLabel: "Replace link",
      });
      if (!ok) return;
    }
    startTransition(async () => {
      try {
        const res = await createPreviewLinkAction(draft.id);
        setLink({ url: `${window.location.origin}${res.path}`, expires_at: res.expires_at });
        toast.success("Private link created. Copy it now; it won't be shown again.");
        router.refresh();
      } catch (e) {
        toast.error(
          toUserMessage(e, "Couldn't create a link.", {
            surface: "admin-drafts",
            action: "preview-link",
          }),
        );
      }
    });
  }

  async function takedown() {
    const ok = await confirm({
      title: `Take down ${draft.display_name}?`,
      description:
        "Deletes the draft and its artworks and stops any link from working. Use this if they said no or stopped replying.",
      confirmLabel: "Take down",
      destructive: true,
    });
    if (!ok) return;
    startTransition(async () => {
      try {
        await takedownDraftAction(draft.id);
        toast.success(`${draft.display_name} taken down`);
        router.refresh();
      } catch (e) {
        toast.error(
          toUserMessage(e, "Couldn't take the draft down.", {
            surface: "admin-drafts",
            action: "takedown",
          }),
        );
      }
    });
  }

  async function copyLink() {
    if (!link) return;
    try {
      await navigator.clipboard.writeText(link.url);
      toast.success("Link copied");
    } catch (e) {
      toast.error(
        toUserMessage(e, "Couldn't copy. Select the link and copy it by hand.", {
          surface: "admin-drafts",
          action: "copy",
        }),
      );
    }
  }

  return (
    <li className="p-4 space-y-3">
      <div className="flex items-start justify-between gap-4 flex-wrap">
        <div className="min-w-0">
          <div className="flex items-baseline gap-2 flex-wrap">
            <Link
              href={`/artists/${encodeURIComponent(draft.slug)}`}
              className="font-serif text-lg hover:underline"
            >
              {draft.display_name}
            </Link>
            <span className="text-xs uppercase tracking-wide text-muted border border-border px-1.5 py-0.5">
              {draft.claimed ? "Claimed" : "Draft"}
            </span>
            {draft.editing && (
              <span className="text-xs uppercase tracking-wide border border-foreground px-1.5 py-0.5">
                Editing
              </span>
            )}
          </div>
          <p className="mt-1 text-xs text-muted">
            {draft.location ?? "no location"} · {draft.artwork_count} artwork
            {draft.artwork_count === 1 ? "" : "s"} · created {formatDate(draft.created_at)}
            {!draft.claimed &&
              (draft.live_link_expires_at
                ? ` · link live until ${formatDate(draft.live_link_expires_at)}`
                : " · no live link")}
          </p>
          <p className="mt-1 text-xs text-muted">Consent: {draft.consent_note}</p>
        </div>

        {!draft.claimed && (
          <div className="flex items-center gap-2 shrink-0 flex-wrap">
            <button type="button" disabled={isPending} onClick={editInStudio} className={BTN_PRIMARY}>
              Edit in studio
            </button>
            <button type="button" disabled={isPending} onClick={mintLink} className={BTN}>
              {draft.live_link_expires_at ? "New private link" : "Create private link"}
            </button>
            <button type="button" disabled={isPending} onClick={takedown} className={BTN}>
              Take down
            </button>
          </div>
        )}
      </div>

      {link && (
        <div className="flex items-center gap-2 flex-wrap">
          <input
            readOnly
            value={link.url}
            onFocus={(e) => e.currentTarget.select()}
            aria-label="Private preview link"
            className="flex-1 min-w-0 bg-background border border-border px-3 py-2 text-xs font-mono"
          />
          <button type="button" onClick={copyLink} className={BTN}>
            Copy
          </button>
          <span className="text-xs text-muted">expires {formatDate(link.expires_at)}</span>
        </div>
      )}
    </li>
  );
}

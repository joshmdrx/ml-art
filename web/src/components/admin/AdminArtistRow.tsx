"use client";

/**
 * T-083 — one row in the `/admin/artists` queue.
 *
 * Displays artist identity + status + the transition affordances
 * applicable to the current status:
 *   - pending: Approve / Decline
 *   - active:  Pause / View
 *   - paused:  Unpause / View
 *   - rejected: (no actions; decline is terminal in v1)
 *
 * Destructive actions go through `useConfirm`; success surfaces via
 * `toast.success`. Errors go to `toast.error` with the API's message.
 * Pattern lifted from `docs/ui-patterns.md`.
 */

import { useTransition } from "react";
import Link from "next/link";
import { useRouter } from "next/navigation";
import { toast } from "sonner";
import { useConfirm } from "@/components/ui/ConfirmDialog";
import { transitionArtist } from "@/app/actions/admin";
import { toUserMessage } from "@/lib/reportError";
import type { AdminArtistItem, AdminArtistTransition } from "@/lib/api";

export function AdminArtistRow({ artist }: { artist: AdminArtistItem }) {
  const router = useRouter();
  const confirm = useConfirm();
  const [isPending, startTransition] = useTransition();

  function run(action: AdminArtistTransition, label: string) {
    startTransition(async () => {
      try {
        await transitionArtist(artist.id, action);
        toast.success(`${artist.display_name}: ${label}`);
        router.refresh();
      } catch (e) {
        toast.error(
          toUserMessage(e, `Couldn't ${label.toLowerCase()} ${artist.display_name}.`, {
            surface: "admin-artists",
            action,
          }),
        );
      }
    });
  }

  async function confirmAndRun(
    action: AdminArtistTransition,
    label: string,
    {
      title,
      description,
      destructive,
    }: { title: string; description: string; destructive?: boolean },
  ) {
    const ok = await confirm({
      title,
      description,
      confirmLabel: label,
      destructive,
    });
    if (ok) run(action, label);
  }

  return (
    <li className="p-4 flex items-start justify-between gap-4">
      <div className="min-w-0">
        <div className="flex items-baseline gap-2 flex-wrap">
          <Link
            href={`/artists/${encodeURIComponent(artist.slug)}`}
            className="font-serif text-lg hover:underline"
          >
            {artist.display_name}
          </Link>
          <span className="text-xs text-muted">/{artist.slug}</span>
        </div>
        <p className="mt-1 text-xs text-muted">
          {artist.email ?? "no Clerk link"} ·{" "}
          {artist.city ?? "no city"}
          {artist.country ? `, ${artist.country}` : ""} ·{" "}
          {artist.artwork_count} artwork
          {artist.artwork_count === 1 ? "" : "s"}
        </p>
      </div>

      <div className="flex items-center gap-2 shrink-0">
        {artist.status === "pending" && (
          <>
            <button
              type="button"
              disabled={isPending}
              onClick={() => run("approve", "Approved")}
              className="px-3 py-1.5 text-sm border border-foreground bg-foreground text-background disabled:opacity-40"
            >
              Approve
            </button>
            <button
              type="button"
              disabled={isPending}
              onClick={() =>
                confirmAndRun("decline", "Decline", {
                  title: `Decline ${artist.display_name}?`,
                  description:
                    "Sets the artist's status to declined. They'll need to be reset to pending before they can be approved later.",
                  destructive: true,
                })
              }
              className="px-3 py-1.5 text-sm border border-border bg-surface hover:bg-background disabled:opacity-40"
            >
              Decline
            </button>
          </>
        )}
        {artist.status === "active" && (
          <button
            type="button"
            disabled={isPending}
            onClick={() =>
              confirmAndRun("pause", "Pause", {
                title: `Pause ${artist.display_name}?`,
                description:
                  "Hides the artist's profile + artworks from public surfaces until unpaused. Their data is preserved.",
                destructive: true,
              })
            }
            className="px-3 py-1.5 text-sm border border-border bg-surface hover:bg-background disabled:opacity-40"
          >
            Pause
          </button>
        )}
        {artist.status === "paused" && (
          <button
            type="button"
            disabled={isPending}
            onClick={() => run("unpause", "Unpaused")}
            className="px-3 py-1.5 text-sm border border-foreground bg-foreground text-background disabled:opacity-40"
          >
            Unpause
          </button>
        )}
      </div>
    </li>
  );
}

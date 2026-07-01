"use client";

/**
 * T-083 — admin preview banner at the top of `/artists/[slug]` when
 * the caller is an admin AND the artist isn't publicly `active`.
 *
 * Puts the queue actions inline with the real page so admins don't
 * have to bounce back to `/admin/artists` between decisions. The
 * status word ("Pending" / "Paused" / "Declined") sits in the
 * banner so it's obvious why the page is visible to admins but
 * not to the public.
 */

import { useTransition } from "react";
import { useRouter } from "next/navigation";
import { toast } from "sonner";
import { useConfirm } from "@/components/ui/ConfirmDialog";
import { transitionArtist } from "@/app/actions/admin";
import { toUserMessage } from "@/lib/reportError";
import type { AdminArtistTransition } from "@/lib/api";

const STATUS_LABEL: Record<string, string> = {
  pending: "Pending review",
  paused: "Paused",
  rejected: "Declined",
};

export function AdminArtistBanner({
  artistId,
  artistName,
  status,
}: {
  artistId: string;
  artistName: string;
  status: string;
}) {
  const router = useRouter();
  const confirm = useConfirm();
  const [isPending, startTransition] = useTransition();

  function run(action: AdminArtistTransition, label: string) {
    startTransition(async () => {
      try {
        await transitionArtist(artistId, action);
        toast.success(`${artistName}: ${label}`);
        router.refresh();
      } catch (e) {
        toast.error(
          toUserMessage(e, `Couldn't ${label.toLowerCase()} ${artistName}.`, {
            surface: "admin-artist-banner",
            action,
          }),
        );
      }
    });
  }

  async function decline() {
    const ok = await confirm({
      title: `Decline ${artistName}?`,
      description:
        "Sets the artist's status to declined. They won't appear publicly. Move them back to pending in the admin queue if they should be reconsidered.",
      confirmLabel: "Decline",
      destructive: true,
    });
    if (ok) run("decline", "Declined");
  }

  return (
    <div className="mb-8 border border-foreground bg-foreground text-background p-4 flex flex-wrap items-center justify-between gap-4">
      <div className="min-w-0">
        <p className="text-xs uppercase tracking-wide opacity-70">
          Admin view
        </p>
        <p className="mt-0.5 text-sm">
          This artist is <strong>{STATUS_LABEL[status] ?? status}</strong> — hidden from the public site.
        </p>
      </div>
      <div className="flex items-center gap-2 shrink-0">
        {status === "pending" && (
          <>
            <button
              type="button"
              disabled={isPending}
              onClick={() => run("approve", "Approved")}
              className="px-3 py-1.5 text-sm bg-background text-foreground disabled:opacity-40"
            >
              Approve
            </button>
            <button
              type="button"
              disabled={isPending}
              onClick={decline}
              className="px-3 py-1.5 text-sm border border-background hover:bg-background/10 disabled:opacity-40"
            >
              Decline
            </button>
          </>
        )}
        {status === "paused" && (
          <button
            type="button"
            disabled={isPending}
            onClick={() => run("unpause", "Unpaused")}
            className="px-3 py-1.5 text-sm bg-background text-foreground disabled:opacity-40"
          >
            Unpause
          </button>
        )}
      </div>
    </div>
  );
}

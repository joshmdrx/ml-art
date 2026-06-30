"use client";

/**
 * T-081.4 — one row in `/admin/venues`.
 *
 * Same shape as `AdminArtistRow` from T-083.2:
 *   - pending_review: Approve / Decline
 *   - active:  no actions in v1 (paused/unpaused not yet wired for venues)
 *   - paused / declined: no actions
 *
 * Approve is one-click (acceptance is the default desire for the
 * platform's growth). Decline goes through useConfirm() since it's
 * the path that turns away an applicant.
 */

import { useTransition } from "react";
import Link from "next/link";
import { useRouter } from "next/navigation";
import { toast } from "sonner";
import { useConfirm } from "@/components/ui/ConfirmDialog";
import { transitionVenue } from "@/app/actions/admin";
import { toUserMessage } from "@/lib/reportError";
import type { AdminVenueItem } from "@/lib/api";

export function AdminVenueRow({ venue }: { venue: AdminVenueItem }) {
  const router = useRouter();
  const confirm = useConfirm();
  const [isPending, startTransition] = useTransition();

  function run(decision: "approve" | "decline", label: string) {
    startTransition(async () => {
      try {
        await transitionVenue(venue.id, decision);
        toast.success(`${venue.name}: ${label}`);
        router.refresh();
      } catch (e) {
        toast.error(
          toUserMessage(e, `Couldn't ${label.toLowerCase()} ${venue.name}.`, {
            surface: "admin-venues",
            decision,
          }),
        );
      }
    });
  }

  async function decline() {
    const ok = await confirm({
      title: `Decline ${venue.name}?`,
      description:
        "Sets the venue's status to declined. They'll stay hidden from public surfaces; an admin can flip them back to pending_review manually if needed.",
      confirmLabel: "Decline",
      destructive: true,
    });
    if (ok) run("decline", "Declined");
  }

  return (
    <li className="p-4 flex items-start justify-between gap-4">
      <div className="min-w-0">
        <div className="flex items-baseline gap-2 flex-wrap">
          <Link
            href={`/venues/${encodeURIComponent(venue.slug)}`}
            className="font-serif text-lg hover:underline"
          >
            {venue.name}
          </Link>
          <span className="text-xs text-muted">/{venue.slug}</span>
        </div>
        <p className="mt-1 text-xs text-muted">
          {venue.owner_email ?? "no Clerk link"} ·{" "}
          {venue.kind.replace("_", " ")}
          {venue.city ? ` · ${venue.city}` : ""}
          {venue.country ? `, ${venue.country}` : ""}
        </p>
      </div>

      {venue.status === "pending_review" && (
        <div className="flex items-center gap-2 shrink-0">
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
            onClick={decline}
            className="px-3 py-1.5 text-sm border border-border bg-surface hover:bg-background disabled:opacity-40"
          >
            Decline
          </button>
        </div>
      )}
    </li>
  );
}

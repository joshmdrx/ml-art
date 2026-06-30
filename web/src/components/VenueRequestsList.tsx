"use client";

/**
 * T-081.2 — artist's venue-invitation inbox.
 *
 * Each pending row has Accept / Decline buttons. Both fire server
 * actions; the row drops out of the list optimistically on success
 * (the API only returns pending rows). Decline goes through
 * `useConfirm` since it's the path that turns down work.
 */

import { useState, useTransition } from "react";
import { useRouter } from "next/navigation";
import { toast } from "sonner";
import { useConfirm } from "@/components/ui/ConfirmDialog";
import { decideVenueRequest } from "@/app/actions/venues";
import { toUserMessage } from "@/lib/reportError";
import type { VenueRequest } from "@/lib/api";

export function VenueRequestsList({
  initialRequests,
}: {
  initialRequests: VenueRequest[];
}) {
  const router = useRouter();
  const confirm = useConfirm();
  const [requests, setRequests] = useState(initialRequests);
  const [isPending, startTransition] = useTransition();

  function key(r: VenueRequest) {
    return `${r.venue_id}:${r.artwork_id}`;
  }

  function accept(r: VenueRequest) {
    startTransition(async () => {
      try {
        await decideVenueRequest(r.venue_id, r.artwork_id, "accept");
        setRequests((prev) => prev.filter((x) => key(x) !== key(r)));
        toast.success(`Accepted — ${r.venue_name} can list this work`);
        router.refresh();
      } catch (e) {
        toast.error(
          toUserMessage(e, "Couldn't accept this request.", {
            surface: "venue-requests",
            action: "accept",
            venue_id: r.venue_id,
            artwork_id: r.artwork_id,
          }),
        );
      }
    });
  }

  async function decline(r: VenueRequest) {
    const ok = await confirm({
      title: `Decline ${r.venue_name}'s request?`,
      description:
        "The venue can re-invite later, which reopens the request to pending. Your artwork won't appear at this venue unless you accept.",
      confirmLabel: "Decline",
      destructive: true,
    });
    if (!ok) return;
    startTransition(async () => {
      try {
        await decideVenueRequest(r.venue_id, r.artwork_id, "decline");
        setRequests((prev) => prev.filter((x) => key(x) !== key(r)));
        toast.success("Declined");
        router.refresh();
      } catch (e) {
        toast.error(
          toUserMessage(e, "Couldn't decline this request.", {
            surface: "venue-requests",
            action: "decline",
            venue_id: r.venue_id,
            artwork_id: r.artwork_id,
          }),
        );
      }
    });
  }

  if (requests.length === 0) {
    return (
      <p className="text-sm text-muted">
        No pending venue requests. Invitations from galleries will show
        up here.
      </p>
    );
  }

  return (
    <ul className="divide-y divide-border border border-border bg-surface">
      {requests.map((r) => (
        <li key={key(r)} className="p-4 flex items-start justify-between gap-4">
          <div className="min-w-0">
            <p className="text-sm">
              <span className="font-serif text-base">{r.venue_name}</span>{" "}
              <span className="text-muted text-xs">
                ({r.venue_kind.replace("_", " ")}
                {r.venue_city ? ` · ${r.venue_city}` : ""}
                {r.venue_country ? `, ${r.venue_country}` : ""})
              </span>
            </p>
            <p className="mt-1 text-xs text-muted">
              wants to show{" "}
              <strong>{r.artwork_title ?? "your untitled work"}</strong>
            </p>
            <p className="mt-1 text-[10px] text-muted">
              Requested{" "}
              <time dateTime={r.requested_at}>
                {new Date(r.requested_at).toLocaleDateString()}
              </time>
            </p>
          </div>
          <div className="flex items-center gap-2 shrink-0">
            <button
              type="button"
              disabled={isPending}
              onClick={() => decline(r)}
              className="px-3 py-1.5 text-sm border border-border bg-background hover:bg-surface disabled:opacity-40"
            >
              Decline
            </button>
            <button
              type="button"
              disabled={isPending}
              onClick={() => accept(r)}
              className="px-3 py-1.5 text-sm border border-foreground bg-foreground text-background disabled:opacity-40"
            >
              Accept
            </button>
          </div>
        </li>
      ))}
    </ul>
  );
}

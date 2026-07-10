"use client";

/**
 * M-08 — admin refund control on `/admin/orders/[id]`. A reason picker
 * plus a destructive-confirm Refund button. Fires `refundOrderAction`;
 * the `charge.refunded` webhook then flips the order to `refunded` and
 * emails the buyer + artist, so on success we just toast + refresh.
 */

import { useState, useTransition } from "react";
import { useRouter } from "next/navigation";
import { toast } from "sonner";
import { useConfirm } from "@/components/ui/ConfirmDialog";
import { refundOrderAction } from "@/app/actions/admin";
import { toUserMessage } from "@/lib/reportError";

const REASONS: Array<{ value: string; label: string }> = [
  { value: "defective", label: "Arrived damaged / defective" },
  { value: "not-as-described", label: "Not as described" },
  { value: "non-delivery", label: "Never arrived" },
  { value: "artist-cancelled", label: "Artist cancelled" },
  { value: "other", label: "Other" },
];

export function RefundButton({ orderId }: { orderId: string }) {
  const router = useRouter();
  const confirm = useConfirm();
  const [reason, setReason] = useState(REASONS[0].value);
  const [isPending, startTransition] = useTransition();

  function onRefund() {
    startTransition(async () => {
      const ok = await confirm({
        title: "Refund this order?",
        description:
          "This refunds the buyer in full, reverses the artist's payout, and returns Wander's commission. It can't be undone.",
        destructive: true,
      });
      if (!ok) return;

      try {
        await refundOrderAction(orderId, reason);
        toast.success("Refund started — the buyer and artist will be notified.");
        router.refresh();
      } catch (e) {
        toast.error(
          toUserMessage(e, "Couldn't refund this order.", {
            surface: "admin-refund",
            orderId,
          })
        );
      }
    });
  }

  return (
    <div className="flex items-center gap-2">
      <select
        value={reason}
        onChange={(e) => setReason(e.target.value)}
        className="bg-background border border-border px-2 py-2 text-sm focus:outline-none focus:border-foreground"
        aria-label="Refund reason"
      >
        {REASONS.map((r) => (
          <option key={r.value} value={r.value}>
            {r.label}
          </option>
        ))}
      </select>
      <button
        type="button"
        onClick={onRefund}
        disabled={isPending}
        className="px-4 py-2 text-sm bg-foreground text-background hover:bg-foreground/90 disabled:opacity-40"
      >
        {isPending ? "Refunding…" : "Refund"}
      </button>
    </div>
  );
}

"use client";

/**
 * Mark-shipped form on `/studio/orders/[id]` (M-06). Captures carrier +
 * tracking and flips the order `paid → shipped` via the `shipOrder`
 * server action. On success: toast + `router.refresh()` so the page
 * re-renders with the new status (the form is only shown while `paid`).
 *
 * JS-only validation via `<FieldError>` — no HTML `required` (per
 * docs/ui-patterns.md).
 */

import { useState, useTransition, type FormEvent } from "react";
import { useRouter } from "next/navigation";
import { toast } from "sonner";
import { FieldError } from "@/components/ui/FieldError";
import { toUserMessage } from "@/lib/reportError";
import { shipOrder } from "@/app/actions/studio";

export function MarkShippedForm({ orderId }: { orderId: string }) {
  const router = useRouter();
  const [carrier, setCarrier] = useState("");
  const [tracking, setTracking] = useState("");
  const [errors, setErrors] = useState<{ carrier?: string; tracking?: string }>(
    {}
  );
  const [isPending, startTransition] = useTransition();

  function onSubmit(e: FormEvent<HTMLFormElement>) {
    e.preventDefault();
    if (isPending) return;

    const next: { carrier?: string; tracking?: string } = {};
    if (!carrier.trim()) next.carrier = "Carrier is required.";
    if (!tracking.trim()) next.tracking = "Tracking number is required.";
    setErrors(next);
    if (Object.keys(next).length > 0) return;

    startTransition(async () => {
      try {
        await shipOrder(orderId, carrier.trim(), tracking.trim());
        toast.success("Marked as shipped — the buyer has been notified.");
        router.refresh();
      } catch (err) {
        toast.error(
          toUserMessage(err, "Couldn't mark as shipped. Please try again.", {
            surface: "mark-shipped",
            orderId,
          })
        );
      }
    });
  }

  return (
    <form onSubmit={onSubmit} className="space-y-4">
      <label className="block">
        <span className="block text-xs text-muted mb-1">Carrier</span>
        <input
          type="text"
          value={carrier}
          onChange={(e) => setCarrier(e.target.value)}
          placeholder="e.g. Royal Mail, DPD"
          className="w-full bg-background border border-border px-3 py-2 text-sm focus:outline-none focus:border-foreground"
        />
        <FieldError message={errors.carrier} />
      </label>
      <label className="block">
        <span className="block text-xs text-muted mb-1">Tracking number</span>
        <input
          type="text"
          value={tracking}
          onChange={(e) => setTracking(e.target.value)}
          maxLength={100}
          className="w-full bg-background border border-border px-3 py-2 text-sm focus:outline-none focus:border-foreground"
        />
        <FieldError message={errors.tracking} />
      </label>
      <button
        type="submit"
        disabled={isPending}
        className="w-full py-3 px-4 bg-foreground text-background text-sm hover:bg-foreground/90 transition-colors disabled:opacity-40"
      >
        {isPending ? "Marking shipped…" : "Mark as shipped"}
      </button>
    </form>
  );
}

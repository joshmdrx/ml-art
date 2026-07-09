"use client";

/**
 * Kicks off (or resumes) Stripe Connect onboarding (M-01/M-06). Calls the
 * `startPayoutOnboarding` server action for a fresh hosted-onboarding URL
 * and redirects the browser to it. The label adapts to onboarding state.
 */

import { useTransition } from "react";
import { toast } from "sonner";
import { toUserMessage } from "@/lib/reportError";
import { startPayoutOnboarding } from "@/app/actions/studio";

export function EnableSalesButton({
  started,
  live,
}: {
  started: boolean;
  live: boolean;
}) {
  const [isPending, startTransition] = useTransition();

  const label = live
    ? "Manage on Stripe"
    : started
      ? "Continue setup"
      : "Set up payouts";

  function onClick() {
    if (isPending) return;
    startTransition(async () => {
      try {
        const { url } = await startPayoutOnboarding();
        window.location.href = url;
      } catch (err) {
        toast.error(
          toUserMessage(err, "Couldn't open Stripe onboarding. Try again.", {
            surface: "enable-sales",
          })
        );
      }
    });
  }

  return (
    <button
      type="button"
      onClick={onClick}
      disabled={isPending}
      className="mt-4 inline-block px-4 py-2 bg-foreground text-background text-sm hover:bg-foreground/90 transition-colors disabled:opacity-40"
    >
      {isPending ? "Opening Stripe…" : label}
    </button>
  );
}

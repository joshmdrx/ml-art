"use client";

/**
 * T-086 — inline "Assign to account" form on a `/admin/drafts` row.
 *
 * Gives the page to an existing Wander account by email, without the
 * organisation needing the link. Also used to fix a claim by the wrong
 * person while the page is still unpublished ("Reassign"), which asks
 * for confirmation first because the current owner loses the page.
 * Refusals (no such account, account already has a page, page already
 * published) render as a `<FieldError>` under the email field.
 */

import { useState, useTransition } from "react";
import { useRouter } from "next/navigation";
import { toast } from "sonner";
import { FieldError } from "@/components/ui/FieldError";
import { useConfirm } from "@/components/ui/ConfirmDialog";
import { assignDraftAction } from "@/app/actions/admin";
import { toUserMessage } from "@/lib/reportError";

const INPUT =
  "flex-1 min-w-0 bg-background border border-border px-3 py-1.5 text-sm focus:outline-none focus:border-foreground";
const BTN =
  "px-3 py-1.5 text-sm border border-border bg-surface hover:bg-background disabled:opacity-40";

export function AssignDraftForm({
  draftId,
  name,
  currentOwner,
  onDone,
}: {
  draftId: string;
  name: string;
  /** Set when reassigning a claimed (still unpublished) page. */
  currentOwner: string | null;
  onDone: () => void;
}) {
  const router = useRouter();
  const confirm = useConfirm();
  const [isPending, startTransition] = useTransition();
  const [email, setEmail] = useState("");
  const [error, setError] = useState<string | null>(null);

  async function onSubmit(e: React.FormEvent) {
    e.preventDefault();
    const value = email.trim();
    if (!value || !value.includes("@")) {
      setError("Enter the email of their Wander account.");
      return;
    }
    setError(null);
    if (currentOwner) {
      const ok = await confirm({
        title: `Move ${name} to ${value}?`,
        description: `${currentOwner} will lose this page. Only do this if the wrong person claimed it.`,
        confirmLabel: "Move page",
        destructive: true,
      });
      if (!ok) return;
    }
    startTransition(async () => {
      try {
        const result = await assignDraftAction(draftId, value);
        if (!result.ok) {
          setError(result.message);
          return;
        }
        toast.success(`${name} now belongs to ${value}`);
        onDone();
        router.refresh();
      } catch (err) {
        setError(
          toUserMessage(err, "Couldn't assign the page. Try again.", {
            surface: "admin-drafts",
            action: "assign",
          }),
        );
      }
    });
  }

  return (
    <form onSubmit={onSubmit} className="space-y-1">
      <div className="flex items-center gap-2 flex-wrap">
        <input
          className={INPUT}
          value={email}
          onChange={(e) => setEmail(e.target.value)}
          placeholder="their Wander account email"
          aria-label="Account email"
        />
        <button type="submit" disabled={isPending} className={BTN}>
          {isPending ? "Assigning…" : currentOwner ? "Move page" : "Assign"}
        </button>
        <button type="button" onClick={onDone} className="text-sm underline hover:no-underline">
          Cancel
        </button>
      </div>
      <FieldError message={error} />
      <p className="text-xs text-muted">
        They need to have signed in to Wander once. Each account can hold one page.
      </p>
    </form>
  );
}

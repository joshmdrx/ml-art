"use client";

/**
 * T-086 — Claim / Remove buttons on `/preview/[token]`.
 *
 * Claim: signed-out visitors go to sign-in (or sign-up) and come back
 * here; signed-in visitors claim via a server action, then land in the
 * studio. Expected refusals (account already has a page, link no
 * longer live) render inline rather than as a toast so they persist.
 * Remove: destructive `useConfirm`, then the page re-renders as the
 * "link no longer works" state.
 */

import { useState, useTransition } from "react";
import Link from "next/link";
import { useRouter } from "next/navigation";
import { toast } from "sonner";
import { useConfirm } from "@/components/ui/ConfirmDialog";
import { claimPreviewAction, removePreviewAction } from "@/app/actions/preview";
import { toUserMessage } from "@/lib/reportError";

const REFUSALS = {
  has_profile:
    "The account you're signed in with already has a page on Wander. Sign out and claim this with a different account, or reply to our email and we'll sort it out.",
  gone: "This link no longer works. It may have expired or been replaced.",
} as const;

export function PreviewActions({
  token,
  name,
  signedIn,
}: {
  token: string;
  name: string;
  signedIn: boolean;
}) {
  const router = useRouter();
  const confirm = useConfirm();
  const [isPending, startTransition] = useTransition();
  const [problem, setProblem] = useState<string | null>(null);
  const returnTo = encodeURIComponent(`/preview/${token}`);

  function claim() {
    setProblem(null);
    startTransition(async () => {
      try {
        const result = await claimPreviewAction(token);
        if (result.ok) {
          toast.success(`${name} is yours. It stays unpublished until you publish it.`);
          router.push("/studio");
          return;
        }
        if (result.reason === "signed_out") {
          router.push(`/sign-in?redirect_url=${returnTo}`);
          return;
        }
        setProblem(REFUSALS[result.reason]);
      } catch (e) {
        setProblem(
          toUserMessage(e, "Couldn't claim the page. Try again in a moment.", {
            surface: "draft-preview",
            action: "claim",
          }),
        );
      }
    });
  }

  async function remove() {
    const ok = await confirm({
      title: "Remove this draft?",
      description:
        "The example page is deleted and this link stops working. Nothing about it was ever public.",
      confirmLabel: "Remove it",
      destructive: true,
    });
    if (!ok) return;
    startTransition(async () => {
      try {
        await removePreviewAction(token);
        toast.success("Removed. Thanks for taking a look.");
        router.refresh();
      } catch (e) {
        setProblem(
          toUserMessage(e, "Couldn't remove the draft. Try again in a moment.", {
            surface: "draft-preview",
            action: "remove",
          }),
        );
      }
    });
  }

  return (
    <div className="mt-5">
      <div className="flex items-center gap-3 flex-wrap">
        {signedIn ? (
          <button
            type="button"
            disabled={isPending}
            onClick={claim}
            className="px-4 py-2 text-sm border border-foreground bg-foreground text-background disabled:opacity-40"
          >
            Claim this page
          </button>
        ) : (
          <>
            <Link
              href={`/sign-up?redirect_url=${returnTo}`}
              className="px-4 py-2 text-sm border border-foreground bg-foreground text-background"
            >
              Create an account to claim it
            </Link>
            <Link
              href={`/sign-in?redirect_url=${returnTo}`}
              className="text-sm underline hover:no-underline"
            >
              I already have an account
            </Link>
          </>
        )}
        <button
          type="button"
          disabled={isPending}
          onClick={remove}
          className="px-4 py-2 text-sm border border-border bg-background hover:bg-surface disabled:opacity-40"
        >
          Remove it
        </button>
      </div>
      {problem && (
        <p role="alert" className="mt-3 p-3 border border-border bg-background text-sm">
          {problem}
        </p>
      )}
    </div>
  );
}

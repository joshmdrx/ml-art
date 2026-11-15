"use client";

/**
 * T-086 — shown across `/studio/*` while an admin is editing a draft
 * profile on an organisation's behalf. Makes it obvious whose page the
 * studio is changing, and offers the way back to the drafts list.
 */

import { useTransition } from "react";
import { useRouter } from "next/navigation";
import { toast } from "sonner";
import { stopEditingDraftAction } from "@/app/actions/admin";
import { toUserMessage } from "@/lib/reportError";

export function EditingDraftBanner({ name }: { name: string }) {
  const router = useRouter();
  const [isPending, startTransition] = useTransition();

  function stop() {
    startTransition(async () => {
      try {
        await stopEditingDraftAction();
        router.push("/admin/drafts");
      } catch (e) {
        toast.error(
          toUserMessage(e, "Couldn't stop editing. Try again.", {
            surface: "studio-draft-banner",
          }),
        );
      }
    });
  }

  return (
    <div
      role="status"
      className="mb-6 p-3 border border-foreground bg-surface text-sm flex items-center justify-between gap-4 flex-wrap"
    >
      <p>
        You&apos;re editing the draft page for <strong>{name}</strong>. It
        stays private until they claim and publish it.
      </p>
      <button
        type="button"
        disabled={isPending}
        onClick={stop}
        className="px-3 py-1.5 text-sm border border-foreground bg-foreground text-background disabled:opacity-40"
      >
        Done editing
      </button>
    </div>
  );
}

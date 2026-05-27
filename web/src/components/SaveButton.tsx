"use client";

/**
 * "Save to collection" button on the artwork detail page.
 *
 *   - Signed in  → opens the SaveModal.
 *   - Signed out → redirects to /sign-in?redirect_url=…
 *
 * Auth state is supplied via Clerk's `useAuth()`. The button shows the same
 * regardless — only the click handler differs.
 */

import { useAuth } from "@clerk/nextjs";
import { useRouter } from "next/navigation";
import { useState } from "react";
import { SaveModal } from "./SaveModal";

export function SaveButton({ artworkId }: { artworkId: string }) {
  const { isSignedIn, isLoaded } = useAuth();
  const router = useRouter();
  const [open, setOpen] = useState(false);

  function onClick() {
    if (!isLoaded) return; // tiny race; ignore the click

    if (!isSignedIn) {
      const next = encodeURIComponent(`/artworks/${artworkId}`);
      router.push(`/sign-in?redirect_url=${next}`);
      return;
    }

    setOpen(true);
  }

  return (
    <>
      <button
        type="button"
        onClick={onClick}
        className="w-full py-3 px-4 border border-border text-sm hover:bg-foreground hover:text-background transition-colors"
      >
        Save to collection
      </button>
      <SaveModal open={open} onOpenChange={setOpen} artworkId={artworkId} />
    </>
  );
}

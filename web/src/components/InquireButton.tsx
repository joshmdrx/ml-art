"use client";

import { useState } from "react";
import { InquiryModal } from "./InquiryModal";

/**
 * Inquire button on the artwork detail page. Opens the InquiryModal,
 * which handles both signed-in and anonymous flows. No sign-in redirect
 * here — anyone (with an email) can inquire.
 */
export function InquireButton({
  artworkId,
  artistName,
}: {
  artworkId: string;
  artistName: string;
}) {
  const [open, setOpen] = useState(false);
  return (
    <>
      <button
        type="button"
        onClick={() => setOpen(true)}
        className="w-full py-3 px-4 bg-foreground text-background text-sm hover:bg-foreground/90 transition-colors"
      >
        Inquire
      </button>
      <InquiryModal
        open={open}
        onOpenChange={setOpen}
        artworkId={artworkId}
        artistName={artistName}
      />
    </>
  );
}

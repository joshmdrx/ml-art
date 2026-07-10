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
  variant = "primary",
}: {
  artworkId: string;
  artistName: string;
  /** "secondary" (outline) when a primary Buy button sits above it. */
  variant?: "primary" | "secondary";
}) {
  const [open, setOpen] = useState(false);
  const className =
    variant === "secondary"
      ? "w-full py-3 px-4 border border-border bg-surface text-sm hover:bg-background transition-colors"
      : "w-full py-3 px-4 bg-foreground text-background text-sm hover:bg-foreground/90 transition-colors";
  return (
    <>
      <button type="button" onClick={() => setOpen(true)} className={className}>
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

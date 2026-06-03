"use client";

/**
 * Camera-icon button that opens a file picker, uploads the chosen
 * image, and navigates to the visual-search results page. Submits via
 * a real `<form action={server-action}>` so the file streams through
 * Next's server-action transport — no client-side fetch wiring, no
 * Bearer-in-the-browser concern.
 *
 * Two sizes match `SearchBar`'s `hero` / `nav`. Hero gets a bigger
 * icon + visible "Search by image" hint label; nav is icon-only.
 */

import { useRef, useState, useTransition } from "react";
import { useFormStatus } from "react-dom";
import { clsx } from "clsx";
import { uploadAndStartVisualSearch } from "@/app/actions/visualSearch";

type Size = "hero" | "nav";

export function VisualSearchUpload({ size }: { size: Size }) {
  const formRef = useRef<HTMLFormElement>(null);
  const inputRef = useRef<HTMLInputElement>(null);
  // Local pending flag — useFormStatus can only be read by descendants
  // of the form, so we mirror it here for the button outside the form.
  const [isPending, startTransition] = useTransition();
  const [error, setError] = useState<string | null>(null);

  function onPick() {
    inputRef.current?.click();
  }

  function onChange(e: React.ChangeEvent<HTMLInputElement>) {
    const file = e.target.files?.[0];
    if (!file) return;
    // Validate before posting so users get instant feedback on the
    // clearly-wrong cases instead of a 400 round-trip.
    if (!file.type.startsWith("image/")) {
      setError("That doesn't look like an image. JPG, PNG, or WebP only.");
      e.target.value = "";
      return;
    }
    if (file.size > 10 * 1024 * 1024) {
      setError("That image is bigger than the 10MB limit.");
      e.target.value = "";
      return;
    }
    setError(null);
    startTransition(() => {
      formRef.current?.requestSubmit();
    });
  }

  return (
    <form
      ref={formRef}
      action={uploadAndStartVisualSearch}
      // React 19 server-action forms manage their own encType + method;
      // setting them explicitly triggers a "you can't override these"
      // console warning. Multipart is implied by the <input type="file">.
      className="inline-flex flex-col items-center"
    >
      <input
        ref={inputRef}
        type="file"
        name="image"
        accept="image/jpeg,image/png,image/webp"
        onChange={onChange}
        className="sr-only"
      />
      <button
        type="button"
        onClick={onPick}
        disabled={isPending}
        aria-label="Search by image"
        title="Search by image"
        className={clsx(
          "inline-flex items-center justify-center border border-border bg-surface hover:bg-background transition-colors disabled:opacity-40",
          size === "hero" ? "h-[58px] px-4 gap-2 text-sm" : "h-[34px] w-[34px]"
        )}
      >
        <CameraIcon
          className={clsx(size === "hero" ? "w-5 h-5" : "w-4 h-4")}
        />
        {size === "hero" && (
          <span>{isPending ? "Uploading…" : "Search by image"}</span>
        )}
      </button>
      {error && (
        <span role="alert" className="mt-2 text-xs text-foreground">
          {error}
        </span>
      )}
      {/* useFormStatus pending lives in this nested submit; the
          button above uses startTransition's flag for the same signal. */}
      <SubmitFallback />
    </form>
  );
}

/** Form-status mirror. Lives inside the form so it can read
 * `useFormStatus` and (visually) is just an SR-only marker. We don't
 * render anything visible from here — the outer button drives the
 * "Uploading…" affordance. */
function SubmitFallback() {
  const { pending } = useFormStatus();
  return (
    <span aria-live="polite" className="sr-only">
      {pending ? "Uploading image, please wait." : ""}
    </span>
  );
}

function CameraIcon({ className }: { className?: string }) {
  // Inline SVG keeps us off heavy icon deps. Same approach as the
  // `×` close button in SaveModal.
  return (
    <svg
      xmlns="http://www.w3.org/2000/svg"
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth="1.5"
      strokeLinecap="round"
      strokeLinejoin="round"
      className={className}
      aria-hidden
    >
      <path d="M4 7h3l2-3h6l2 3h3a1 1 0 0 1 1 1v11a1 1 0 0 1-1 1H4a1 1 0 0 1-1-1V8a1 1 0 0 1 1-1z" />
      <circle cx="12" cy="13" r="4" />
    </svg>
  );
}

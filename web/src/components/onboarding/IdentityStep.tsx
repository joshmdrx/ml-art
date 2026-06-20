"use client";

/**
 * T-012 Phase 1 — onboarding step 1: identity.
 *
 * Captures `display_name` (required) + `location` (optional, free-text).
 * Submits to `POST /v1/onboarding/start` via the `startOnboarding`
 * server action; on success, navigates to `?step=profile`.
 */

import { useState, useTransition, type FormEvent } from "react";
import { useRouter } from "next/navigation";
import { startOnboarding } from "@/app/actions/onboarding";
import { toUserMessage } from "@/lib/reportError";

export function IdentityStep() {
  const router = useRouter();
  const [displayName, setDisplayName] = useState("");
  const [location, setLocation] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [isPending, startTransition] = useTransition();

  function onSubmit(e: FormEvent<HTMLFormElement>) {
    e.preventDefault();
    setError(null);
    if (displayName.trim().length === 0) {
      setError("Display name is required.");
      return;
    }
    startTransition(async () => {
      try {
        await startOnboarding({
          display_name: displayName.trim(),
          location: location.trim() || undefined,
        });
        router.push("/onboarding?step=profile");
      } catch (e) {
        setError(
          toUserMessage(e, "Couldn't start onboarding. Try again.", {
            surface: "onboarding-identity",
          }),
        );
      }
    });
  }

  return (
    <form onSubmit={onSubmit} className="space-y-5 max-w-xl">
      <div>
        <h2 className="font-serif text-2xl tracking-tight">
          Let&apos;s start with who you are
        </h2>
        <p className="mt-2 text-sm text-muted">
          You can edit any of this later in your studio settings.
        </p>
      </div>

      <label className="block text-sm">
        <span className="block mb-1 text-muted">
          Display name
          <span aria-hidden className="text-red-600 ml-1">
            *
          </span>
        </span>
        <input
          type="text"
          value={displayName}
          onChange={(e) => setDisplayName(e.target.value)}
          required
          maxLength={100}
          autoFocus
          placeholder="Jane Doe"
          className="w-full border border-border bg-bg px-3 py-2"
        />
        <span className="mt-1 block text-xs text-muted">
          We&apos;ll generate a profile URL from this — `/artists/jane-doe`.
        </span>
      </label>

      <label className="block text-sm">
        <span className="block mb-1 text-muted">Location (optional)</span>
        <input
          type="text"
          value={location}
          onChange={(e) => setLocation(e.target.value)}
          maxLength={200}
          placeholder="Berlin, Germany"
          className="w-full border border-border bg-bg px-3 py-2"
        />
      </label>

      {error && <p className="text-sm text-red-600">{error}</p>}

      <div className="flex items-center justify-end pt-2">
        <button
          type="submit"
          disabled={isPending || displayName.trim().length === 0}
          className="text-sm px-4 py-2 bg-fg text-bg disabled:opacity-50"
        >
          {isPending ? "Saving…" : "Continue"}
        </button>
      </div>
    </form>
  );
}

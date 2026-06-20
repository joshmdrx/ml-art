"use client";

/**
 * T-012 Phase 1 — onboarding step 2: profile.
 *
 * Bio + statement + website. Submits via the existing
 * `updateStudioSettings` server action (same endpoint the settings
 * page uses), then advances to step 3.
 *
 * Skippable — none of these are required to publish. The "Skip for
 * now" link is a plain navigation to the next step.
 */

import { useState, useTransition, type FormEvent } from "react";
import Link from "next/link";
import { useRouter } from "next/navigation";
import { updateStudioSettings } from "@/app/actions/studio";
import { normalizeWebsiteUrl } from "@/lib/normalizeUrl";
import { toUserMessage } from "@/lib/reportError";
import type { StudioArtist, StudioSettingsPatch } from "@/lib/api";

interface Props {
  initial: StudioArtist;
}

export function ProfileStep({ initial }: Props) {
  const router = useRouter();
  const [bio, setBio] = useState(initial.bio ?? "");
  const [statement, setStatement] = useState(initial.artist_statement ?? "");
  const [websiteUrl, setWebsiteUrl] = useState(initial.website_url ?? "");
  const [error, setError] = useState<string | null>(null);
  const [isPending, startTransition] = useTransition();

  function buildPatch(): StudioSettingsPatch {
    const patch: StudioSettingsPatch = {};
    const eq = (a: string, b: string | null) =>
      a.trim() === (b ?? "").trim();
    if (!eq(bio, initial.bio)) patch.bio = bio.trim() || null;
    if (!eq(statement, initial.artist_statement))
      patch.artist_statement = statement.trim() || null;
    // Normalize bare hostnames ("guitardojo.app") to a real URL with
    // a scheme before sending — the server validator requires it,
    // and artists shouldn't have to type `https://`.
    const normalizedUrl = normalizeWebsiteUrl(websiteUrl);
    const previousUrl = initial.website_url?.trim() || null;
    if (normalizedUrl !== previousUrl) {
      patch.website_url = normalizedUrl;
    }
    return patch;
  }

  function onSubmit(e: FormEvent<HTMLFormElement>) {
    e.preventDefault();
    setError(null);
    const patch = buildPatch();
    if (Object.keys(patch).length === 0) {
      // Nothing changed; just advance.
      router.push("/onboarding?step=artworks");
      return;
    }
    startTransition(async () => {
      try {
        await updateStudioSettings(patch);
        router.push("/onboarding?step=artworks");
      } catch (e) {
        setError(
          toUserMessage(e, "Couldn't save your profile. Try again.", {
            surface: "onboarding-profile",
          }),
        );
      }
    });
  }

  return (
    <form onSubmit={onSubmit} className="space-y-5 max-w-2xl">
      <div>
        <h2 className="font-serif text-2xl tracking-tight">
          Tell collectors about your work
        </h2>
        <p className="mt-2 text-sm text-muted">
          All optional. You can come back and edit these any time.
        </p>
      </div>

      <label className="block text-sm">
        <span className="block mb-1 text-muted">Bio</span>
        <textarea
          value={bio}
          onChange={(e) => setBio(e.target.value)}
          rows={3}
          maxLength={4000}
          placeholder="A few sentences about you and your practice."
          className="w-full border border-border bg-bg px-3 py-2"
        />
      </label>

      <label className="block text-sm">
        <span className="block mb-1 text-muted">Artist statement</span>
        <textarea
          value={statement}
          onChange={(e) => setStatement(e.target.value)}
          rows={6}
          maxLength={8000}
          placeholder="Longer-form: themes, materials, what you're exploring."
          className="w-full border border-border bg-bg px-3 py-2"
        />
      </label>

      <label className="block text-sm">
        <span className="block mb-1 text-muted">Website</span>
        <input
          type="text"
          inputMode="url"
          autoCapitalize="none"
          autoCorrect="off"
          spellCheck={false}
          value={websiteUrl}
          onChange={(e) => setWebsiteUrl(e.target.value)}
          maxLength={500}
          placeholder="yoursite.com"
          className="w-full border border-border bg-bg px-3 py-2"
        />
      </label>

      {error && <p className="text-sm text-red-600">{error}</p>}

      <div className="flex items-center justify-between pt-2">
        <Link
          href="/onboarding?step=identity"
          className="text-sm text-muted hover:text-foreground"
        >
          ← Back
        </Link>
        <div className="flex items-center gap-3">
          <Link
            href="/onboarding?step=artworks"
            className="text-sm text-muted hover:text-foreground"
          >
            Skip for now
          </Link>
          <button
            type="submit"
            disabled={isPending}
            className="text-sm px-4 py-2 bg-fg text-bg disabled:opacity-50"
          >
            {isPending ? "Saving…" : "Save and continue"}
          </button>
        </div>
      </div>
    </form>
  );
}

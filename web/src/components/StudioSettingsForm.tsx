"use client";

/**
 * Studio settings form. Single column, no tabs. Submits a PATCH body
 * derived by diffing the form state against `initial` — sending only
 * the keys the user actually touched, which keeps the server action's
 * partial-update semantics honest and gives the artist a clean "unsaved
 * changes" cue (Save button stays disabled when nothing changed).
 *
 * Visibility toggle is its own thing — flips `status` between `active`
 * (Published) and `paused` (Unpublished). Persisted on click, not
 * batched with the rest of the form, so the artist gets immediate
 * "hide me" without filling the rest of the page.
 */

import { useState, useTransition, type FormEvent } from "react";
import { useRouter } from "next/navigation";
import { clsx } from "clsx";
import { updateStudioSettings } from "@/app/actions/studio";
import type { StudioArtist, StudioSettingsPatch } from "@/lib/api";

interface Props {
  initial: StudioArtist;
}

export function StudioSettingsForm({ initial }: Props) {
  const router = useRouter();
  const [bio, setBio] = useState(initial.bio ?? "");
  const [statement, setStatement] = useState(initial.artist_statement ?? "");
  const [location, setLocation] = useState(initial.location ?? "");
  const [websiteUrl, setWebsiteUrl] = useState(initial.website_url ?? "");
  const [status, setStatus] = useState<"active" | "paused">(
    initial.status === "paused" ? "paused" : "active"
  );
  const [error, setError] = useState<string | null>(null);
  const [savedFlash, setSavedFlash] = useState(false);
  const [isPending, startTransition] = useTransition();

  // Diff the form against `initial` so the PATCH body only carries
  // fields the user actually changed. Treats whitespace-equal as
  // unchanged so trimming doesn't dirty the form.
  function buildPatch(): StudioSettingsPatch {
    const patch: StudioSettingsPatch = {};
    const eq = (a: string, b: string | null) =>
      a.trim() === (b ?? "").trim();
    if (!eq(bio, initial.bio)) patch.bio = bio.trim() || null;
    if (!eq(statement, initial.artist_statement))
      patch.artist_statement = statement.trim() || null;
    if (!eq(location, initial.location)) patch.location = location.trim() || null;
    if (!eq(websiteUrl, initial.website_url))
      patch.website_url = websiteUrl.trim() || null;
    return patch;
  }

  const dirty = Object.keys(buildPatch()).length > 0;

  function onSubmit(e: FormEvent<HTMLFormElement>) {
    e.preventDefault();
    if (!dirty || isPending) return;
    setError(null);
    setSavedFlash(false);
    const patch = buildPatch();
    startTransition(async () => {
      try {
        await updateStudioSettings(patch);
        setSavedFlash(true);
        router.refresh();
      } catch (err) {
        setError(err instanceof Error ? err.message : String(err));
      }
    });
  }

  function toggleVisibility() {
    if (isPending) return;
    const next = status === "active" ? "paused" : "active";
    setError(null);
    setSavedFlash(false);
    startTransition(async () => {
      try {
        await updateStudioSettings({ status: next });
        setStatus(next);
        setSavedFlash(true);
        router.refresh();
      } catch (err) {
        setError(err instanceof Error ? err.message : String(err));
      }
    });
  }

  return (
    <form
      onSubmit={onSubmit}
      className={clsx("space-y-8", isPending && "opacity-70")}
    >
      {error && (
        <p
          role="alert"
          className="p-3 border border-border bg-surface text-sm"
        >
          Save failed: <code className="font-mono">{error}</code>
        </p>
      )}

      {savedFlash && !error && (
        <p role="status" className="text-sm text-muted">
          Saved.
        </p>
      )}

      <Field
        label="Bio"
        hint="A paragraph or two — appears on your public artist page."
      >
        <textarea
          value={bio}
          onChange={(e) => setBio(e.target.value)}
          rows={4}
          maxLength={4_000}
          className="w-full bg-background border border-border px-3 py-2 text-sm focus:outline-none focus:border-foreground"
        />
      </Field>

      <Field
        label="Artist statement"
        hint="What are you trying to do with your work?"
      >
        <textarea
          value={statement}
          onChange={(e) => setStatement(e.target.value)}
          rows={6}
          maxLength={8_000}
          className="w-full bg-background border border-border px-3 py-2 text-sm focus:outline-none focus:border-foreground"
        />
      </Field>

      <Field
        label="Location"
        hint="Free-form. We re-geocode this in the background; clearing the field hides location from your profile."
      >
        <input
          type="text"
          value={location}
          onChange={(e) => setLocation(e.target.value)}
          maxLength={200}
          placeholder="London, GB"
          className="w-full bg-background border border-border px-3 py-2 text-sm focus:outline-none focus:border-foreground"
        />
      </Field>

      <Field label="Website" hint="Optional; must start with http:// or https://.">
        <input
          type="url"
          value={websiteUrl}
          onChange={(e) => setWebsiteUrl(e.target.value)}
          maxLength={500}
          placeholder="https://example.com"
          className="w-full bg-background border border-border px-3 py-2 text-sm focus:outline-none focus:border-foreground"
        />
      </Field>

      <div className="flex items-center gap-3">
        <button
          type="submit"
          disabled={!dirty || isPending}
          className="px-5 py-2 text-sm bg-foreground text-background disabled:opacity-40"
        >
          {isPending ? "Saving…" : "Save changes"}
        </button>
        {!dirty && !isPending && (
          <span className="text-xs text-muted">No unsaved changes.</span>
        )}
      </div>

      {/* Visibility section sits at the bottom because it persists
          independently of the form-wide Save button. */}
      <section
        aria-labelledby="visibility-heading"
        className="mt-12 pt-8 border-t border-border"
      >
        <h2 id="visibility-heading" className="font-serif text-xl">
          Portfolio visibility
        </h2>
        <p className="mt-2 text-sm text-muted leading-relaxed">
          {status === "active"
            ? "Your portfolio is public. Search, artist pages, and neighborhoods all surface your work."
            : "Your portfolio is hidden. Your artworks won't appear in search or any other public surface until you re-publish. Nothing is deleted — re-publishing restores everything."}
        </p>
        <button
          type="button"
          onClick={toggleVisibility}
          disabled={isPending}
          aria-pressed={status === "paused"}
          className={clsx(
            "mt-4 px-5 py-2 text-sm border",
            status === "active"
              ? "border-border bg-surface hover:bg-background"
              : "border-foreground bg-foreground text-background"
          )}
        >
          {status === "active" ? "Unpublish portfolio" : "Re-publish portfolio"}
        </button>
      </section>
    </form>
  );
}

function Field({
  label,
  hint,
  children,
}: {
  label: string;
  hint?: string;
  children: React.ReactNode;
}) {
  return (
    <label className="block">
      <span className="block font-medium text-sm mb-1">{label}</span>
      {hint && <span className="block text-xs text-muted mb-2">{hint}</span>}
      {children}
    </label>
  );
}

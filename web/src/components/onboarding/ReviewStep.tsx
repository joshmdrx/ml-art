"use client";

/**
 * T-012 Phase 1 — onboarding step 5: review and publish.
 *
 * Last step. Shows a summary of what's been filled in and a Publish
 * button that calls `POST /v1/onboarding/complete` to flip
 * `status: pending → active` and then redirects to the artist's
 * public portfolio.
 *
 * If the artist is *already* active (e.g. they navigated here after
 * publishing once), we don't surface the Publish button — instead, a
 * "Go to studio" link.
 */

import { useState, useTransition } from "react";
import Link from "next/link";
import { useRouter } from "next/navigation";
import { completeOnboarding } from "@/app/actions/onboarding";
import { toUserMessage } from "@/lib/reportError";
import type { StudioArtist, StudioArtworkSummary, StudioLocation } from "@/lib/api";

interface Props {
  artist: StudioArtist;
  artworks: StudioArtworkSummary[];
  locations: StudioLocation[];
}

export function ReviewStep({ artist, artworks, locations }: Props) {
  const router = useRouter();
  const [error, setError] = useState<string | null>(null);
  const [isPending, startTransition] = useTransition();
  const alreadyActive = artist.status === "active";

  function onPublish() {
    setError(null);
    startTransition(async () => {
      try {
        await completeOnboarding();
        // Redirect to the public profile so the artist sees their work
        // the way a visitor will.
        router.push(`/artists/${artist.slug}`);
      } catch (e) {
        setError(
          toUserMessage(e, "Couldn't publish your studio. Try again.", {
            surface: "onboarding-publish",
          }),
        );
      }
    });
  }

  return (
    <section className="space-y-6 max-w-2xl">
      <div>
        <h2 className="font-serif text-2xl tracking-tight">Ready to publish?</h2>
        <p className="mt-2 text-sm text-muted">
          Here&apos;s what your profile will look like. You can edit anything
          later from your studio.
        </p>
      </div>

      <dl className="border border-border bg-surface divide-y divide-border">
        <Row label="Name" value={artist.display_name} />
        <Row label="URL" value={`/artists/${artist.slug}`} mono />
        <Row label="Location" value={artist.location ?? "—"} />
        <Row
          label="Bio"
          value={artist.bio ? truncate(artist.bio, 160) : "—"}
        />
        <Row
          label="Artworks"
          value={`${artworks.length} ${artworks.length === 1 ? "piece" : "pieces"}`}
        />
        <Row
          label="Venues"
          value={`${locations.length} ${locations.length === 1 ? "location" : "locations"}`}
        />
      </dl>

      {error && <p className="text-sm text-red-600">{error}</p>}

      {alreadyActive ? (
        <div className="flex items-center justify-between pt-2">
          <Link
            href="/onboarding?step=locations"
            className="text-sm text-muted hover:text-foreground"
          >
            ← Back
          </Link>
          <Link
            href={`/artists/${artist.slug}`}
            className="text-sm px-4 py-2 bg-fg text-bg"
          >
            View your profile →
          </Link>
        </div>
      ) : (
        <div className="flex items-center justify-between pt-2">
          <Link
            href="/onboarding?step=locations"
            className="text-sm text-muted hover:text-foreground"
          >
            ← Back
          </Link>
          <button
            type="button"
            onClick={onPublish}
            disabled={isPending}
            className="text-sm px-4 py-2 bg-fg text-bg disabled:opacity-50"
          >
            {isPending ? "Publishing…" : "Publish profile"}
          </button>
        </div>
      )}
    </section>
  );
}

function Row({
  label,
  value,
  mono,
}: {
  label: string;
  value: string;
  mono?: boolean;
}) {
  return (
    <div className="px-4 py-3 grid grid-cols-3 gap-3 text-sm">
      <dt className="text-muted">{label}</dt>
      <dd className={`col-span-2 ${mono ? "font-mono text-xs" : ""}`}>
        {value}
      </dd>
    </div>
  );
}

function truncate(s: string, n: number): string {
  return s.length > n ? `${s.slice(0, n - 1)}…` : s;
}

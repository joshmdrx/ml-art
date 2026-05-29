"use client";

/**
 * T-012 Phase 1 — onboarding step 4: where to see your work.
 *
 * Thin wrapper around `StudioLocationsManager` (T-038 G3) so the
 * onboarding flow uses the exact same CRUD surface as the studio
 * settings page. Locations are entirely optional; the "Continue"
 * button is enabled with zero rows.
 */

import Link from "next/link";
import { StudioLocationsManager } from "@/components/StudioLocationsManager";
import type { StudioLocation } from "@/lib/api";

interface Props {
  initial: StudioLocation[];
}

export function LocationsStep({ initial }: Props) {
  return (
    <section className="space-y-6 max-w-3xl">
      <div>
        <h2 className="font-serif text-2xl tracking-tight">
          Where can people see your work in person?
        </h2>
        <p className="mt-2 text-sm text-muted">
          Add galleries you&apos;re represented by, or your studio if you take
          visitors. Pins show on your public profile and on the search map.
          Entirely optional — skip if you&apos;d rather come back to it.
        </p>
      </div>

      {/* The Manager has its own headings + UX; we present it raw
          rather than re-styling. */}
      <div className="-mt-12">
        <StudioLocationsManager initial={initial} />
      </div>

      <div className="flex items-center justify-between pt-4 border-t border-border">
        <Link
          href="/onboarding?step=artworks"
          className="text-sm text-muted hover:text-foreground"
        >
          ← Back
        </Link>
        <Link
          href="/onboarding?step=review"
          className="text-sm px-4 py-2 bg-fg text-bg"
        >
          Continue
        </Link>
      </div>
    </section>
  );
}

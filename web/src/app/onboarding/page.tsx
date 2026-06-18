import type { Metadata } from "next";
import { redirect } from "next/navigation";
import { auth } from "@clerk/nextjs/server";
import { TopNav } from "@/components/TopNav";
import {
  StepNav,
  STEP_ORDER,
  type OnboardingStep,
} from "@/components/onboarding/StepNav";
import { IdentityStep } from "@/components/onboarding/IdentityStep";
import { ProfileStep } from "@/components/onboarding/ProfileStep";
import { ArtworksStep } from "@/components/onboarding/ArtworksStep";
import { LocationsStep } from "@/components/onboarding/LocationsStep";
import { ReviewStep } from "@/components/onboarding/ReviewStep";
import {
  getStudioMe,
  listMyArtworks,
  listStudioLocations,
} from "@/lib/api";
import { reportError } from "@/lib/reportError";

export const metadata: Metadata = {
  title: "Onboarding",
};

/**
 * /onboarding — multi-step wizard for new artists (T-012 Phase 1).
 *
 * States:
 *  - signed-out → redirect to /sign-in with a return URL
 *  - signed-in, no artist row → only step=identity is valid
 *  - signed-in, pending artist → any step is reachable; "Publish"
 *    button in the review step flips status to active
 *  - signed-in, active artist → all steps reachable, review step
 *    becomes "View your profile" (re-entering the wizard to edit is
 *    fine; you just can't unpublish from here — use studio settings)
 *
 * Step is taken from `?step=…`. Defaults to identity (or the next
 * step the artist hasn't completed yet).
 */

type Search = Promise<{ step?: string }>;

function parseStep(raw: string | undefined): OnboardingStep | null {
  return STEP_ORDER.includes(raw as OnboardingStep)
    ? (raw as OnboardingStep)
    : null;
}

export default async function OnboardingPage({
  searchParams,
}: {
  searchParams: Search;
}) {
  const { userId } = await auth();
  if (!userId) {
    redirect("/sign-in?redirect_url=" + encodeURIComponent("/onboarding"));
  }

  const { step: rawStep } = await searchParams;

  // Load the current artist (may be null if not yet onboarded). All
  // subsequent gating depends on this.
  let artist;
  try {
    artist = await getStudioMe();
  } catch (e) {
    reportError(e, { surface: "onboarding-load-artist" });
    artist = null;
  }

  // Gate to the right step. The identity step is the only one that
  // works without an artist row.
  const requested = parseStep(rawStep);
  if (!artist) {
    // No artist yet — force identity.
    if (requested !== null && requested !== "identity") {
      redirect("/onboarding?step=identity");
    }
    return (
      <Shell current="identity" furthest="identity">
        <IdentityStep />
      </Shell>
    );
  }

  // Artist exists. The default step is `profile` (we just minted them)
  // unless the caller asked for something else.
  const current: OnboardingStep = requested ?? "profile";
  // For now, "furthest" is just "review" once we have an artist —
  // any step is reachable. A future enhancement could track partial
  // progress per-step.
  const furthest: OnboardingStep = "review";

  if (current === "identity") {
    // They already onboarded; rewrite to a real step rather than
    // showing the identity form again (which would 400).
    redirect("/onboarding?step=profile");
  }

  if (current === "profile") {
    return (
      <Shell current={current} furthest={furthest}>
        <ProfileStep initial={artist} />
      </Shell>
    );
  }

  if (current === "artworks") {
    const list = await listMyArtworks({ status: "all" }).catch((e) => {
      reportError(e, { surface: "onboarding-load-artworks" });
      return null;
    });
    return (
      <Shell current={current} furthest={furthest}>
        <ArtworksStep artist={artist} items={list?.items ?? []} />
      </Shell>
    );
  }

  if (current === "locations") {
    const locs = await listStudioLocations().catch((e) => {
      reportError(e, { surface: "onboarding-load-locations" });
      return null;
    });
    return (
      <Shell current={current} furthest={furthest}>
        <LocationsStep initial={locs ?? []} />
      </Shell>
    );
  }

  // review
  const [list, locs] = await Promise.all([
    listMyArtworks({ status: "all" }).catch((e) => {
      reportError(e, { surface: "onboarding-load-artworks" });
      return null;
    }),
    listStudioLocations().catch((e) => {
      reportError(e, { surface: "onboarding-load-locations" });
      return null;
    }),
  ]);
  return (
    <Shell current={current} furthest={furthest}>
      <ReviewStep
        artist={artist}
        artworks={list?.items ?? []}
        locations={locs ?? []}
      />
    </Shell>
  );
}

function Shell({
  current,
  furthest,
  children,
}: {
  current: OnboardingStep;
  furthest: OnboardingStep;
  children: React.ReactNode;
}) {
  return (
    <>
      <TopNav />
      <main className="flex-1 mx-auto w-full max-w-3xl px-6 py-12">
        <header className="mb-8">
          <p className="text-xs uppercase tracking-wider text-muted">
            Onboarding
          </p>
          <h1 className="font-serif text-3xl tracking-tight">
            Set up your portfolio
          </h1>
        </header>
        <StepNav current={current} furthest={furthest} />
        {children}
      </main>
    </>
  );
}

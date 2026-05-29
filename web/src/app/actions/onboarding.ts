"use server";

/**
 * Server actions for the `/onboarding` wizard (T-012 Phase 1).
 *
 * Each mutating step submits through a server action so the Clerk
 * Bearer never touches the browser — same pattern as
 * `actions/studio.ts` and `actions/collections.ts`. The actions wrap
 * the corresponding `lib/api.ts` calls and revalidate the paths whose
 * cached payload depends on the artist's new state.
 */

import { revalidatePath } from "next/cache";
import {
  completeOnboarding as apiCompleteOnboarding,
  startOnboarding as apiStartOnboarding,
  type StartOnboardingBody,
  type StudioArtist,
} from "@/lib/api";

export async function startOnboarding(
  body: StartOnboardingBody
): Promise<StudioArtist> {
  const a = await apiStartOnboarding(body);
  // The artist row is brand-new; the only path that already cached an
  // empty `/v1/studio/me` is /onboarding itself.
  revalidatePath("/onboarding");
  revalidatePath("/studio");
  return a;
}

export async function completeOnboarding(): Promise<StudioArtist> {
  const a = await apiCompleteOnboarding();
  // Going pending → active makes the artist visible on every public
  // surface. Revalidate the slug-keyed pages so they re-render with
  // the new status.
  revalidatePath("/onboarding");
  revalidatePath("/studio");
  revalidatePath("/studio/settings");
  revalidatePath(`/artists/${a.slug}`);
  return a;
}

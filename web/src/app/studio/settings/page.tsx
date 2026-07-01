import type { Metadata } from "next";
import { redirect } from "next/navigation";
import { auth } from "@clerk/nextjs/server";
import { StudioSettingsForm } from "@/components/StudioSettingsForm";
import { StudioLocationsManager } from "@/components/StudioLocationsManager";
import { getStudioMe, listStudioLocations } from "@/lib/api";
import { reportError } from "@/lib/reportError";

export const metadata: Metadata = {
  title: "Studio settings",
};

/**
 * `/studio/settings` — the artist's profile + visibility controls.
 *
 * States:
 *   - signed-out → /sign-in?redirect_url=/studio/settings
 *   - signed-in non-artist → /onboarding (self-serve mint flow,
 *     T-012 Phase 1)
 *   - signed-in artist → form pre-filled with `getStudioMe()` result
 */
export default async function StudioSettingsPage() {
  const { userId } = await auth();
  if (!userId) {
    redirect("/sign-in?redirect_url=" + encodeURIComponent("/studio/settings"));
  }

  let artist;
  try {
    artist = await getStudioMe();
  } catch (e) {
    reportError(e, { surface: "studio-settings" });
    artist = null;
  }
  if (!artist) {
    redirect("/onboarding");
  }

  // Locations: same null-collapses-failure pattern as the rest of the
  // studio surface.
  let locations: Awaited<ReturnType<typeof listStudioLocations>> = null;
  try {
    locations = await listStudioLocations();
  } catch (e) {
    reportError(e, { surface: "studio-locations" });
  }

  return (
    <div className="max-w-3xl">
      <header className="mb-8">
        <h1 className="font-serif text-3xl tracking-tight">Settings</h1>
        <p className="mt-2 text-sm text-muted">
          Edit your profile, statement, and visibility.
        </p>
      </header>

      <StudioSettingsForm initial={artist} />
      <StudioLocationsManager initial={locations ?? []} />
    </div>
  );
}

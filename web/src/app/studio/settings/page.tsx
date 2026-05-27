import type { Metadata } from "next";
import Link from "next/link";
import { redirect } from "next/navigation";
import { auth } from "@clerk/nextjs/server";
import { TopNav } from "@/components/TopNav";
import { StudioSettingsForm } from "@/components/StudioSettingsForm";
import { getStudioMe } from "@/lib/api";
import { reportError } from "@/lib/reportError";

export const metadata: Metadata = {
  title: "Studio settings — ml-art",
};

/**
 * `/studio/settings` — the artist's profile + visibility controls.
 *
 * Three states:
 *   - signed-out → /sign-in?redirect_url=/studio/settings
 *   - signed-in non-artist (no `artists.user_id` link) → "you're not an
 *     artist yet" empty state (onboarding lands as `T-012`)
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

  return (
    <>
      <TopNav />
      <main className="flex-1 mx-auto w-full max-w-3xl px-6 py-12">
        <header className="mb-8">
          <h1 className="font-serif text-3xl tracking-tight">Studio settings</h1>
          <p className="mt-2 text-sm text-muted">
            Edit your profile, statement, and visibility.
          </p>
        </header>

        {artist ? (
          <StudioSettingsForm initial={artist} />
        ) : (
          <NotAnArtistYet />
        )}
      </main>
    </>
  );
}

function NotAnArtistYet() {
  return (
    <section className="p-6 border border-border bg-surface">
      <h2 className="font-serif text-xl">You&apos;re not set up as an artist yet.</h2>
      <p className="mt-3 text-sm leading-relaxed">
        Studio settings are for verified artists with a portfolio on the
        platform. We&apos;re currently onboarding artists by direct invitation
        only — if you think this is wrong,{" "}
        <Link href="/" className="underline">
          head back to the homepage
        </Link>{" "}
        or get in touch.
      </p>
    </section>
  );
}

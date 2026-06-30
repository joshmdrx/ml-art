import type { Metadata } from "next";
import Link from "next/link";
import { redirect } from "next/navigation";
import { auth } from "@clerk/nextjs/server";
import { TopNav } from "@/components/TopNav";
import { StudioVenuesManager } from "@/components/StudioVenuesManager";
import { getMe, listStudioVenues } from "@/lib/api";
import { reportError } from "@/lib/reportError";

export const metadata: Metadata = {
  title: "Venues — Studio",
};

/**
 * `/studio/venues` — T-081.2.
 *
 * Authenticated user's venues (a single user can own zero, one, or
 * many — the studio side surfaces all of them regardless of approval
 * status). Manage existing + open the create modal from here.
 *
 * Unlike `/studio/series`, the calling user does NOT need to be an
 * artist — gallery owners are users without an artist row. Gate on
 * sign-in only.
 */
export default async function StudioVenuesPage() {
  const { userId } = await auth();
  if (!userId) {
    redirect("/sign-in?redirect_url=" + encodeURIComponent("/studio/venues"));
  }

  const [me, venues] = await Promise.all([
    getMe().catch((e) => {
      reportError(e, { surface: "studio-venues", call: "me" });
      return null;
    }),
    listStudioVenues().catch((e) => {
      reportError(e, { surface: "studio-venues", call: "list" });
      return null;
    }),
  ]);

  if (!me) {
    redirect("/sign-in?redirect_url=" + encodeURIComponent("/studio/venues"));
  }

  const items = venues ?? [];

  return (
    <>
      <TopNav />
      <main className="flex-1 mx-auto w-full max-w-screen-2xl px-6 py-12">
        <header className="flex items-baseline justify-between mb-8">
          <div>
            <h1 className="font-serif text-3xl tracking-tight">Venues</h1>
            <p className="mt-2 text-sm text-muted">
              Galleries, shops, or studio collectives you own. Invite
              artworks to be shown at your venue; the artist accepts
              or declines.
            </p>
          </div>
          <Link
            href="/studio"
            className="text-sm underline underline-offset-2 text-muted hover:text-foreground"
          >
            ← Back to portfolio
          </Link>
        </header>

        <StudioVenuesManager initialVenues={items} />
      </main>
    </>
  );
}

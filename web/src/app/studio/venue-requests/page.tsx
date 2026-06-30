import type { Metadata } from "next";
import Link from "next/link";
import { redirect } from "next/navigation";
import { auth } from "@clerk/nextjs/server";
import { TopNav } from "@/components/TopNav";
import { VenueRequestsList } from "@/components/VenueRequestsList";
import { getStudioMe, listVenueRequests } from "@/lib/api";
import { reportError } from "@/lib/reportError";

export const metadata: Metadata = {
  title: "Venue requests — Studio",
};

/**
 * `/studio/venue-requests` — T-081.2.
 *
 * Artist's inbox of pending invitations from venues. Accept lets the
 * venue list the artwork publicly; decline marks the row but leaves
 * the audit trail intact (the venue can re-invite later, which flips
 * it back to pending).
 *
 * Restricted to artists — non-artist users get redirected to /studio
 * (which itself redirects to /onboarding for non-artists). The page
 * is empty for artists with no pending invitations.
 */
export default async function VenueRequestsPage() {
  const { userId } = await auth();
  if (!userId) {
    redirect(
      "/sign-in?redirect_url=" +
        encodeURIComponent("/studio/venue-requests"),
    );
  }

  const [artist, requests] = await Promise.all([
    getStudioMe().catch((e) => {
      reportError(e, { surface: "venue-requests", call: "me" });
      return null;
    }),
    listVenueRequests().catch((e) => {
      reportError(e, { surface: "venue-requests", call: "list" });
      return null;
    }),
  ]);

  if (!artist) {
    redirect("/onboarding");
  }

  const items = requests ?? [];

  return (
    <>
      <TopNav />
      <main className="flex-1 mx-auto w-full max-w-screen-lg px-6 py-12">
        <header className="flex items-baseline justify-between mb-8">
          <div>
            <h1 className="font-serif text-3xl tracking-tight">
              Venue requests
            </h1>
            <p className="mt-2 text-sm text-muted">
              Galleries and shops asking to show your work. Accept to
              have your artwork listed at that venue&apos;s public
              page; decline to leave it unlisted.
            </p>
          </div>
          <Link
            href="/studio"
            className="text-sm underline underline-offset-2 text-muted hover:text-foreground"
          >
            ← Back to portfolio
          </Link>
        </header>

        <VenueRequestsList initialRequests={items} />
      </main>
    </>
  );
}

import Link from "next/link";
import { auth } from "@clerk/nextjs/server";
import { TopNav } from "@/components/TopNav";
import { NearMeButton } from "@/components/NearMeButton";
import { SearchBar } from "@/components/SearchBar";
import { VisualSearchUpload } from "@/components/VisualSearchUpload";
import { ArtworkGrid } from "@/components/ArtworkGrid";
import { NeighborhoodCard } from "@/components/NeighborhoodCard";
import { CalibratePanel } from "@/components/CalibratePanel";
import {
  searchArtworks,
  listNeighborhoods,
  getCalibratePairs,
  getForYou,
} from "@/lib/api";
import { reportError } from "@/lib/reportError";

export default async function Home() {
  // T-056.2 — fetch "For you" alongside the rest, but only for
  // signed-in users. Anonymous callers would get an empty 401 →
  // skipping the request saves a round-trip on the (still common)
  // cold-visit path.
  const { userId } = await auth();
  const forYouPromise = userId
    ? getForYou().catch((e) => {
        reportError(e, { surface: "home", feed: "for-you" });
        return { items: [], eligible: false };
      })
    : Promise.resolve({ items: [], eligible: false });

  // Fetch the homepage feeds in parallel. Calibrator pairs SSR'd
  // alongside the rest — the panel decides client-side whether to
  // render based on a localStorage flag, so we don't waste cycles
  // on returning visitors but the data's ready if they're new.
  const [recent, neighborhoods, calibratePairs, forYou] = await Promise.all([
    searchArtworks({ sort: "newest", limit: 12 }).catch((e) => {
      reportError(e, { surface: "home", feed: "recent" });
      return { items: [], next_cursor: null };
    }),
    listNeighborhoods().catch((e) => {
      reportError(e, { surface: "home", feed: "neighborhoods" });
      return { items: [], next_cursor: null };
    }),
    getCalibratePairs().catch((e) => {
      reportError(e, { surface: "home", feed: "calibrate" });
      return { pairs: [] };
    }),
    forYouPromise,
  ]);

  // First user-visible payoff of the T-055 + T-061 chain: when the
  // user clears the eligibility gate, swap the generic "Recently
  // added" row for personalised retrieval. Otherwise the current
  // surface stays intact — cold-start visitors and signed-out users
  // see exactly what they did before.
  const showForYou = forYou.eligible && forYou.items.length > 0;

  return (
    <>
      <TopNav hideSearch />

      <main className="flex-1">
        {/* Hero search — text + visual side-by-side. */}
        <section className="py-24 md:py-32 flex flex-col items-center gap-4 px-6">
          <div className="flex w-full max-w-2xl items-stretch gap-2">
            <div className="flex-1">
              <SearchBar size="hero" />
            </div>
            <VisualSearchUpload size="hero" />
          </div>
          {/* Map discovery affordances (T-042 + T-043). The NearMe
              button self-hides when geolocation is unavailable, so
              the row collapses cleanly on unsupported browsers. */}
          <div className="flex items-center gap-3 text-sm">
            <NearMeButton variant="hero" />
            <span className="text-muted">or</span>
            <Link
              href="/search?map=1"
              className="inline-flex items-center border border-border bg-surface hover:bg-fg/10 px-4 py-2"
            >
              Explore the map →
            </Link>
          </div>
        </section>

        {/* T-061 calibrator. Self-hides if the visitor has already
            completed/skipped, or if the corpus has no semantic
            neighbourhoods yet. Sits above the rest so it's visible
            without scroll for new visitors. */}
        <CalibratePanel pairs={calibratePairs.pairs} />

        {/* Neighborhoods */}
        {neighborhoods.items.length > 0 && (
          <section className="mx-auto max-w-screen-2xl px-6 pb-20">
            <div className="flex items-baseline justify-between mb-8">
              <h2 className="font-serif text-2xl">Explore neighborhoods</h2>
              <Link
                href="/neighborhoods"
                className="text-sm text-muted hover:text-foreground"
              >
                See all →
              </Link>
            </div>
            <div className="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-3 gap-6">
              {neighborhoods.items.slice(0, 6).map((n) => (
                <NeighborhoodCard key={n.id} neighborhood={n} />
              ))}
            </div>
          </section>
        )}

        {/* For-you / Recently-added — same slot. Picked by eligibility:
            signed-in users with a taste vector + ≥5 interactions see
            their personalised row; everyone else sees the generic
            "Recently added". */}
        <section className="mx-auto max-w-screen-2xl px-6 pb-24">
          {showForYou ? (
            <>
              <div className="flex items-baseline justify-between mb-8">
                <h2 className="font-serif text-2xl">For you</h2>
                <span className="text-xs text-muted">
                  Tuned from what you&apos;ve told us so far.
                </span>
              </div>
              <ArtworkGrid items={forYou.items} />
            </>
          ) : (
            <>
              <h2 className="font-serif text-2xl mb-8">Recently added</h2>
              {recent.items.length === 0 ? (
                <p className="text-muted text-sm">
                  Nothing here yet. Try{" "}
                  <Link
                    href="/search"
                    className="underline hover:text-foreground"
                  >
                    searching
                  </Link>{" "}
                  or{" "}
                  <Link
                    href="/neighborhoods"
                    className="underline hover:text-foreground"
                  >
                    exploring by neighborhood
                  </Link>
                  .
                </p>
              ) : (
                <ArtworkGrid items={recent.items} />
              )}
            </>
          )}
        </section>
      </main>

      <footer className="border-t border-border py-8 text-xs text-muted">
        <div className="mx-auto max-w-screen-2xl px-6 flex flex-wrap gap-6">
          <span>Wander</span>
          <a href="/about" className="hover:text-foreground">About</a>
          <a href="/for-artists" className="hover:text-foreground">For Artists</a>
          <a href="/privacy" className="hover:text-foreground">Privacy</a>
          <a href="/terms" className="hover:text-foreground">Terms</a>
        </div>
      </footer>
    </>
  );
}

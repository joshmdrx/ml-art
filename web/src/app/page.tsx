import Link from "next/link";
import { TopNav } from "@/components/TopNav";
import { SearchBar } from "@/components/SearchBar";
import { ArtworkGrid } from "@/components/ArtworkGrid";
import { NeighborhoodCard } from "@/components/NeighborhoodCard";
import { searchArtworks, listNeighborhoods } from "@/lib/api";
import { reportError } from "@/lib/reportError";

export default async function Home() {
  // Fetch the homepage feeds in parallel.
  const [recent, neighborhoods] = await Promise.all([
    searchArtworks({ sort: "newest", limit: 12 }).catch((e) => {
      reportError(e, { surface: "home", feed: "recent" });
      return { items: [], next_cursor: null };
    }),
    listNeighborhoods().catch((e) => {
      reportError(e, { surface: "home", feed: "neighborhoods" });
      return { items: [], next_cursor: null };
    }),
  ]);

  return (
    <>
      <TopNav hideSearch />

      <main className="flex-1">
        {/* Hero search */}
        <section className="py-24 md:py-32 flex justify-center px-6">
          <SearchBar size="hero" />
        </section>

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

        {/* Recently added */}
        <section className="mx-auto max-w-screen-2xl px-6 pb-24">
          <h2 className="font-serif text-2xl mb-8">Recently added</h2>
          {recent.items.length === 0 ? (
            <p className="text-muted text-sm">
              No artworks yet — make sure the API is running on{" "}
              <code className="font-mono">localhost:9100</code> and the
              seed has been applied.
            </p>
          ) : (
            <ArtworkGrid items={recent.items} />
          )}
        </section>
      </main>

      <footer className="border-t border-border py-8 text-xs text-muted">
        <div className="mx-auto max-w-screen-2xl px-6 flex flex-wrap gap-6">
          <span>ml-art</span>
          <a href="/about" className="hover:text-foreground">About</a>
          <a href="/for-artists" className="hover:text-foreground">For Artists</a>
          <a href="/privacy" className="hover:text-foreground">Privacy</a>
          <a href="/terms" className="hover:text-foreground">Terms</a>
        </div>
      </footer>
    </>
  );
}

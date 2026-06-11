import { TopNav } from "@/components/TopNav";
import { NeighborhoodCard } from "@/components/NeighborhoodCard";
import { listNeighborhoods } from "@/lib/api";
import { reportError } from "@/lib/reportError";
import type { Metadata } from "next";

export const metadata: Metadata = {
  title: "Neighborhoods — Wander",
  description: "Clusters of visually and conceptually related work.",
};

export default async function NeighborhoodsIndex() {
  let resp;
  try {
    resp = await listNeighborhoods();
  } catch (e) {
    reportError(e, { surface: "neighborhoods-index" });
    resp = { items: [], next_cursor: null };
  }

  return (
    <>
      <TopNav />
      <main className="flex-1 mx-auto w-full max-w-screen-2xl px-6 py-12 md:py-16">
        <header className="mb-10 md:mb-14 max-w-2xl">
          <h1 className="font-serif text-4xl md:text-5xl tracking-tight">
            Neighborhoods
          </h1>
          <p className="mt-3 text-sm text-muted">
            Clusters of visually and conceptually related work. Hand-curated
            for now; algorithmic clusters land once the corpus is large
            enough to be meaningful.
          </p>
        </header>

        {resp.items.length === 0 ? (
          <p className="text-muted text-sm">No neighborhoods yet.</p>
        ) : (
          <div className="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-3 gap-6">
            {resp.items.map((n) => (
              <NeighborhoodCard key={n.id} neighborhood={n} />
            ))}
          </div>
        )}
      </main>
    </>
  );
}

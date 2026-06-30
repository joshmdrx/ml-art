import type { Metadata } from "next";
import Link from "next/link";
import { TopNav } from "@/components/TopNav";
import { listPublicVenues } from "@/lib/api";
import { reportError } from "@/lib/reportError";

export const metadata: Metadata = {
  title: "Venues — Wander",
  description:
    "Galleries, shops, and collectives showing work by artists on Wander.",
};

type Search = { city?: string; cursor?: string };

export default async function VenuesIndex({
  searchParams,
}: {
  searchParams: Promise<Search>;
}) {
  const sp = await searchParams;
  const page = await listPublicVenues({
    city: sp.city,
    cursor: sp.cursor,
  }).catch((e) => {
    reportError(e, { surface: "venues-index" });
    return null;
  });
  const items = page?.items ?? [];

  return (
    <>
      <TopNav />
      <main className="flex-1 mx-auto w-full max-w-screen-2xl px-6 py-12">
        <header className="mb-8">
          <h1 className="font-serif text-3xl tracking-tight">Venues</h1>
          <p className="mt-2 text-sm text-muted">
            Galleries, shops, and studio collectives showing work by
            artists on Wander.
            {sp.city && (
              <>
                {" "}
                Filtered to <strong>{sp.city}</strong>.{" "}
                <Link
                  href="/venues"
                  className="underline underline-offset-2"
                >
                  Clear
                </Link>
              </>
            )}
          </p>
        </header>

        {items.length === 0 ? (
          <p className="text-sm text-muted">
            {sp.city
              ? "No venues in this city yet."
              : "No venues have been approved yet."}
          </p>
        ) : (
          <ul className="grid grid-cols-1 sm:grid-cols-2 md:grid-cols-3 gap-4">
            {items.map((v) => (
              <li key={v.id}>
                <Link
                  href={`/venues/${encodeURIComponent(v.slug)}`}
                  className="block border border-border bg-surface p-5 hover:bg-background transition-colors"
                >
                  <h2 className="font-serif text-xl line-clamp-1">{v.name}</h2>
                  <p className="mt-1 text-xs text-muted">
                    {v.kind.replace("_", " ")}
                    {v.city ? ` · ${v.city}` : ""}
                    {v.country ? `, ${v.country}` : ""}
                  </p>
                  {v.website_url && (
                    <p className="mt-2 text-xs text-muted line-clamp-1">
                      {v.website_url.replace(/^https?:\/\//, "")}
                    </p>
                  )}
                </Link>
              </li>
            ))}
          </ul>
        )}

        {page?.next_cursor && (
          <div className="mt-8 text-center">
            <Link
              href={`/venues?cursor=${page.next_cursor}${sp.city ? `&city=${encodeURIComponent(sp.city)}` : ""}`}
              className="text-sm text-muted hover:text-foreground underline underline-offset-2"
            >
              Next page →
            </Link>
          </div>
        )}
      </main>
    </>
  );
}

import { notFound } from "next/navigation";
import type { Metadata } from "next";
import Link from "next/link";
import { TopNav } from "@/components/TopNav";
import { ArtworkGrid } from "@/components/ArtworkGrid";
import { getPublicCollection } from "@/lib/api";
import { reportError } from "@/lib/reportError";

// T-053 — public read view of a shared collection.
//
// No auth required. The share token in the URL is the entire credential.
// Renders the collection without any save / edit / inquire-from-here
// affordances — this is purely the "look at someone's mood board" view.

type Params = Promise<{ share_id: string }>;

export async function generateMetadata({
  params,
}: {
  params: Params;
}): Promise<Metadata> {
  const { share_id } = await params;
  const data = await getPublicCollection(share_id).catch(() => null);
  if (!data) return { title: "Collection" };
  return {
    title: data.collection.name,
    description:
      data.collection.description ??
      `A collection of ${data.collection.artwork_count} works on Wander.`,
  };
}

export default async function PublicCollectionPage({
  params,
}: {
  params: Params;
}) {
  const { share_id } = await params;

  const data = await getPublicCollection(share_id).catch((e) => {
    reportError(e, { surface: "public-collection", share_id });
    return null;
  });
  if (!data) notFound();

  const { collection, artworks } = data;

  return (
    <>
      <TopNav />
      <main className="flex-1 mx-auto w-full max-w-screen-2xl px-6 py-12 md:py-16">
        <header className="mb-10 md:mb-14 max-w-3xl">
          <p className="text-xs text-muted tracking-wide uppercase mb-3">
            A collection on Wander
          </p>
          <h1 className="font-serif text-4xl md:text-5xl tracking-tight">
            {collection.name}
          </h1>
          {collection.description && (
            <p className="mt-4 text-base leading-relaxed">
              {collection.description}
            </p>
          )}
          <p className="mt-3 text-xs text-muted">
            {collection.artwork_count}{" "}
            {collection.artwork_count === 1 ? "work" : "works"}
          </p>
        </header>

        {artworks.items.length === 0 ? (
          <p className="text-sm text-muted">
            This collection is empty.
          </p>
        ) : (
          <ArtworkGrid items={artworks.items} />
        )}

        <p className="mt-16 text-xs text-muted">
          <Link href="/" className="hover:text-foreground">
            ← Wander
          </Link>
        </p>
      </main>
    </>
  );
}

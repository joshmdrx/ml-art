import { notFound, redirect } from "next/navigation";
import type { Metadata } from "next";
import Link from "next/link";
import { auth } from "@clerk/nextjs/server";
import { TopNav } from "@/components/TopNav";
import { ArtworkGrid } from "@/components/ArtworkGrid";
import { getCollection } from "@/lib/api";
import { reportError } from "@/lib/reportError";

type Params = Promise<{ id: string }>;

export async function generateMetadata({
  params,
}: {
  params: Params;
}): Promise<Metadata> {
  const { id } = await params;
  const data = await getCollection(id).catch(() => null);
  if (!data) return { title: "Collection — Wander" };
  return {
    title: `${data.collection.name} — Wander`,
    description: data.collection.description ?? undefined,
  };
}

export default async function CollectionPage({ params }: { params: Params }) {
  const { id } = await params;

  const { userId } = await auth();
  if (!userId) {
    redirect(
      "/sign-in?redirect_url=" + encodeURIComponent(`/collections/${id}`)
    );
  }

  const data = await getCollection(id).catch((e) => {
    reportError(e, { surface: "collection-detail", id });
    return null;
  });
  if (!data) notFound();

  const { collection, artworks } = data;

  return (
    <>
      <TopNav />
      <main className="flex-1 mx-auto w-full max-w-screen-2xl px-6 py-12 md:py-16">
        <header className="mb-10 md:mb-14 max-w-3xl">
          <h1 className="font-serif text-4xl md:text-5xl tracking-tight">
            {collection.name}
          </h1>
          {collection.description && (
            <p className="mt-4 text-base leading-relaxed">
              {collection.description}
            </p>
          )}
          <p className="mt-3 text-xs text-muted flex items-center gap-2">
            <span>{collection.artwork_count} works</span>
            {collection.is_public && (
              <span className="px-1.5 py-0.5 border border-border">
                Public
              </span>
            )}
          </p>
        </header>

        {artworks.items.length === 0 ? (
          <p className="text-sm text-muted">
            Nothing saved here yet. Click <em>Save to collection</em> from any
            artwork to add it.
          </p>
        ) : (
          <ArtworkGrid items={artworks.items} />
        )}

        <p className="mt-16 text-xs text-muted">
          <Link href="/collections" className="hover:text-foreground">
            ← All collections
          </Link>
        </p>
      </main>
    </>
  );
}

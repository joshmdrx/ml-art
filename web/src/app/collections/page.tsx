import { redirect } from "next/navigation";
import type { Metadata } from "next";
import Link from "next/link";
import { auth } from "@clerk/nextjs/server";
import { TopNav } from "@/components/TopNav";
import { listMyCollections } from "@/lib/api";
import type { CollectionSummary } from "@/lib/api";

export const metadata: Metadata = {
  title: "Your collections — ml-art",
};

export default async function CollectionsPage() {
  // Server-side auth gate — anonymous users get bounced to sign-in.
  const { userId } = await auth();
  if (!userId) {
    redirect("/sign-in?redirect_url=" + encodeURIComponent("/collections"));
  }

  const resp = await listMyCollections();

  return (
    <>
      <TopNav />
      <main className="flex-1 mx-auto w-full max-w-screen-2xl px-6 py-12 md:py-16">
        <header className="mb-10 md:mb-14 max-w-2xl">
          <h1 className="font-serif text-4xl md:text-5xl tracking-tight">
            Your collections
          </h1>
          <p className="mt-3 text-sm text-muted">
            Save artworks while you browse — they collect here. Make a
            collection public to share a link.
          </p>
        </header>

        {resp.items.length === 0 ? (
          <p className="text-sm text-muted">
            You haven&apos;t saved any artworks yet.{" "}
            <Link href="/search" className="underline hover:no-underline">
              Find some
            </Link>
            .
          </p>
        ) : (
          <div className="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-3 gap-6">
            {resp.items.map((c) => (
              <CollectionCard key={c.id} collection={c} />
            ))}
          </div>
        )}
      </main>
    </>
  );
}

function CollectionCard({ collection }: { collection: CollectionSummary }) {
  // Cover: asymmetric 4-thumb mosaic (1 tall + 3 small). Fills with empty
  // boxes if there are fewer images.
  const slots: (string | undefined)[] = [0, 1, 2, 3].map(
    (i) => collection.cover_image_urls[i]
  );

  return (
    <Link
      href={`/collections/${collection.id}`}
      className="group block bg-surface border border-border p-4 hover:border-foreground/30 transition-colors"
    >
      <div className="grid grid-cols-3 grid-rows-3 gap-1 h-48 mb-4">
        <Thumb src={slots[0]} className="row-span-3 col-span-2" />
        <Thumb src={slots[1]} />
        <Thumb src={slots[2]} />
        <Thumb src={slots[3]} />
      </div>
      <h2 className="font-serif text-lg truncate">{collection.name}</h2>
      <p className="mt-1 text-xs text-muted flex items-center gap-2">
        <span>{collection.artwork_count} works</span>
        {collection.is_public && (
          <span className="px-1.5 py-0.5 border border-border">Public</span>
        )}
      </p>
    </Link>
  );
}

function Thumb({
  src,
  className = "",
}: {
  src: string | undefined;
  className?: string;
}) {
  return (
    <div className={`bg-border overflow-hidden ${className}`}>
      {src && (
        // eslint-disable-next-line @next/next/no-img-element
        <img
          src={src}
          alt=""
          loading="lazy"
          className="w-full h-full object-cover"
        />
      )}
    </div>
  );
}

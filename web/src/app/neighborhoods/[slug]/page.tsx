import { notFound } from "next/navigation";
import type { Metadata } from "next";
import Link from "next/link";
import { TopNav } from "@/components/TopNav";
import { ArtworkGrid } from "@/components/ArtworkGrid";
import { FilterBar } from "@/components/FilterBar";
import { getNeighborhood, type NeighborhoodFilters } from "@/lib/api";
import { priceParamsFromToken } from "@/lib/filterBar";
import { reportError } from "@/lib/reportError";

type Params = Promise<{ slug: string }>;
type Search = {
  medium?: string;
  price?: string;
  price_min?: string;
  price_max?: string;
  availability?: string;
};

export async function generateMetadata({
  params,
}: {
  params: Params;
}): Promise<Metadata> {
  const { slug } = await params;
  const data = await getNeighborhood(slug).catch(() => null);
  if (!data) return { title: "Neighborhood not found — Wander" };
  const n = data.neighborhood;
  return {
    title: `${n.name} — Neighborhoods — Wander`,
    description: n.description ?? `Works in the ${n.name} neighborhood.`,
  };
}

export default async function NeighborhoodPage({
  params,
  searchParams,
}: {
  params: Params;
  searchParams: Promise<Search>;
}) {
  const { slug } = await params;
  const sp = await searchParams;

  const bucketPrice = priceParamsFromToken(sp.price);
  const filters: NeighborhoodFilters = {
    medium: sp.medium?.trim() || undefined,
    price_min:
      bucketPrice?.price_min ??
      (sp.price_min ? Number(sp.price_min) : undefined),
    price_max:
      bucketPrice?.price_max ??
      (sp.price_max ? Number(sp.price_max) : undefined),
    availability: sp.availability?.trim() || undefined,
  };

  const data = await getNeighborhood(slug, filters).catch((e) => {
    reportError(e, { surface: "neighborhood-detail", slug });
    return null;
  });
  if (!data) notFound();

  const { neighborhood, artworks } = data;

  return (
    <>
      <TopNav />
      <main className="flex-1 mx-auto w-full max-w-screen-2xl px-6 py-12 md:py-16">
        <header className="mb-10 md:mb-14 max-w-3xl">
          <h1 className="font-serif text-4xl md:text-5xl tracking-tight">
            {neighborhood.name}
          </h1>
          {neighborhood.description && (
            <p className="mt-4 text-base leading-relaxed">
              {neighborhood.description}
            </p>
          )}
          <p className="mt-3 text-xs text-muted">
            {neighborhood.artwork_count} works
          </p>

          {neighborhood.representative_image_urls.length > 0 && (
            <div className="mt-8 flex gap-3 overflow-x-auto">
              {neighborhood.representative_image_urls.map((url, i) => (
                // eslint-disable-next-line @next/next/no-img-element
                <img
                  key={i}
                  src={url}
                  alt=""
                  className="h-32 w-auto border border-border"
                />
              ))}
            </div>
          )}
        </header>

        <section>
          <FilterBar
            availableFilters={["medium", "price", "availability"]}
            basePath={`/neighborhoods/${slug}`}
          />
          {artworks.items.length === 0 ? (
            <p className="text-muted text-sm">
              No artworks match these filters.
            </p>
          ) : (
            <ArtworkGrid items={artworks.items} />
          )}
        </section>

        <p className="mt-16 text-xs text-muted">
          <Link href="/neighborhoods" className="hover:text-foreground">
            ← All neighborhoods
          </Link>
        </p>
      </main>
    </>
  );
}

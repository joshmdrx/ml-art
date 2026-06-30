import type { Metadata } from "next";
import Link from "next/link";
import { notFound } from "next/navigation";
import { TopNav } from "@/components/TopNav";
import { getPublicVenue } from "@/lib/api";

type Params = Promise<{ slug: string }>;

export async function generateMetadata({
  params,
}: {
  params: Params;
}): Promise<Metadata> {
  const { slug } = await params;
  const v = await getPublicVenue(slug).catch(() => null);
  if (!v) return { title: "Venue not found — Wander" };
  return {
    title: `${v.name} — Wander`,
    description:
      v.about ??
      `${v.kind.replace("_", " ")} in ${v.city ?? "an unspecified city"}.`,
  };
}

export default async function VenueDetailPage({
  params,
}: {
  params: Params;
}) {
  const { slug } = await params;
  const venue = await getPublicVenue(slug).catch(() => null);
  if (!venue) notFound();

  return (
    <>
      <TopNav />
      <main className="flex-1 mx-auto w-full max-w-screen-2xl px-6 py-12">
        <p className="text-xs text-muted mb-2">
          <Link href="/venues" className="hover:text-foreground">
            ← Venues
          </Link>
        </p>

        <header className="mb-10">
          <h1 className="font-serif text-4xl tracking-tight">{venue.name}</h1>
          <p className="mt-2 text-sm text-muted">
            {venue.kind.replace("_", " ")}
            {venue.city ? ` · ${venue.city}` : ""}
            {venue.country ? `, ${venue.country}` : ""}
          </p>
          {venue.about && (
            <p className="mt-6 max-w-prose text-sm leading-relaxed">
              {venue.about}
            </p>
          )}
          <dl className="mt-6 grid grid-cols-1 sm:grid-cols-2 gap-x-6 gap-y-2 text-sm max-w-prose">
            {venue.address && (
              <>
                <dt className="text-muted">Address</dt>
                <dd>{venue.address}</dd>
              </>
            )}
            {venue.opening_info && (
              <>
                <dt className="text-muted">Hours</dt>
                <dd>{venue.opening_info}</dd>
              </>
            )}
            {venue.website_url && (
              <>
                <dt className="text-muted">Website</dt>
                <dd>
                  <a
                    href={venue.website_url}
                    target="_blank"
                    rel="noopener noreferrer"
                    className="underline underline-offset-2"
                  >
                    {venue.website_url.replace(/^https?:\/\//, "")}
                  </a>
                </dd>
              </>
            )}
            {venue.instagram_handle && (
              <>
                <dt className="text-muted">Instagram</dt>
                <dd>{venue.instagram_handle}</dd>
              </>
            )}
          </dl>
        </header>

        {venue.artworks.length > 0 && (
          <section>
            <h2 className="font-serif text-2xl mb-4">Currently showing</h2>
            <ul className="grid grid-cols-1 sm:grid-cols-2 md:grid-cols-3 lg:grid-cols-4 gap-4">
              {venue.artworks.map((a) => (
                <li key={a.artwork_id}>
                  <Link
                    href={`/artworks/${encodeURIComponent(a.artwork_id)}`}
                    className="block border border-border bg-surface hover:bg-background"
                  >
                    <div className="aspect-square bg-background overflow-hidden">
                      {a.primary_image_url ? (
                        // eslint-disable-next-line @next/next/no-img-element
                        <img
                          src={a.primary_image_url}
                          alt={a.title ?? ""}
                          loading="lazy"
                          className="w-full h-full object-cover"
                        />
                      ) : null}
                    </div>
                    <div className="p-3">
                      <p className="text-sm line-clamp-1">
                        {a.title ?? <em className="text-muted">untitled</em>}
                      </p>
                      <p className="mt-0.5 text-xs text-muted line-clamp-1">
                        by {a.artist_display_name}
                      </p>
                    </div>
                  </Link>
                </li>
              ))}
            </ul>
          </section>
        )}
      </main>
    </>
  );
}

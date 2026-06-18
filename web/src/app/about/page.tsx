import Link from "next/link";
import type { Metadata } from "next";
import { TopNav } from "@/components/TopNav";

export const metadata: Metadata = {
  title: "About",
  description:
    "Wander is a discovery platform for independent contemporary artists. No marketplace. No commissions. Just better search.",
};

export default function AboutPage() {
  return (
    <>
      <TopNav />
      <main className="flex-1 mx-auto w-full max-w-3xl px-6 py-12 md:py-16">
        <header className="mb-10">
          <h1 className="font-serif text-4xl md:text-5xl tracking-tight">
            About Wander
          </h1>
          <p className="mt-3 text-sm text-muted">
            A discovery platform for independent contemporary artists.
          </p>
        </header>

        <article className="space-y-6 text-[15px] leading-relaxed">
          <p>
            Most ways to find art online want to sell it to you. They
            tag work by colour and price, push you toward whatever is
            trending, and bury everything else. The artists who don&apos;t
            fit the algorithm don&apos;t get found.
          </p>

          <p>
            Wander is the opposite shape. We don&apos;t sell anything. We
            don&apos;t take a commission. We just help people who care
            about art find artists who deserve more attention — and let
            them get in touch directly.
          </p>

          <h2 className="font-serif text-2xl pt-4">What&apos;s different</h2>

          <p>
            <strong>Search by what the work looks like.</strong> Drop in
            an image and find visually similar pieces across every artist
            on the platform. It works on style, palette, composition, and
            subject — not just tags.
          </p>

          <p>
            <strong>Neighborhoods, not categories.</strong> Curated
            clusters that group work by shared sensibility. A starting
            point for someone who doesn&apos;t know an artist&apos;s
            name yet.
          </p>

          <p>
            <strong>Map mode.</strong> See where artists work. Find a
            studio you could actually visit.
          </p>

          <h2 className="font-serif text-2xl pt-4">For artists</h2>

          <p>
            A studio on Wander is free and stays free. We don&apos;t
            take a cut of anything that happens after a buyer finds you.{" "}
            <Link
              href="/for-artists"
              className="underline hover:text-foreground"
            >
              More about how it works for artists →
            </Link>
          </p>

          <h2 className="font-serif text-2xl pt-4">Where we are</h2>

          <p>
            Early. The catalogue is seeded with a public-domain corpus
            so the search and discovery surfaces have something to chew
            on; real artists are arriving. If you make work and want a
            studio, sign up — we&apos;re onboarding people one at a
            time and answering email at{" "}
            <a
              href="mailto:info@wander.gallery"
              className="underline hover:text-foreground"
            >
              info@wander.gallery
            </a>
            .
          </p>
        </article>
      </main>
    </>
  );
}

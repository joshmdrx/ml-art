import Link from "next/link";
import type { Metadata } from "next";
import { TopNav } from "@/components/TopNav";

export const metadata: Metadata = {
  title: "About",
  description:
    "Wander helps you find independent contemporary art you can see in person: search by image, mood or phrase, and find studios and galleries on the map.",
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
            Wander is the opposite shape. We help people who care about
            art find artists and spaces that deserve more attention —
            ideally somewhere they can go and see the work in person —
            and let them get in touch directly.
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
            <strong>Map mode.</strong> See where artists work and where
            their work is showing. Find a studio or gallery you could
            actually visit.
          </p>

          <h2 className="font-serif text-2xl pt-4">For artists</h2>

          <p>
            A studio on Wander is free and stays free. Enquiries go
            straight to you, and we never take a cut of a sale you
            arrange yourself. If you&apos;d like buyers to check out on
            Wander instead, you can turn on direct sales: we take 15% of
            sales made through Wander, and nothing on anything else.{" "}
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

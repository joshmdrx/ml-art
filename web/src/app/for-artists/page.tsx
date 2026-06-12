import Link from "next/link";
import type { Metadata } from "next";
import { TopNav } from "@/components/TopNav";

export const metadata: Metadata = {
  title: "For artists — Wander",
  description:
    "A free studio on Wander. No marketplace, no commissions, no algorithm pushing you down the feed.",
};

export default function ForArtistsPage() {
  return (
    <>
      <TopNav />
      <main className="flex-1 mx-auto w-full max-w-3xl px-6 py-12 md:py-16">
        <header className="mb-10">
          <h1 className="font-serif text-4xl md:text-5xl tracking-tight">
            A free studio.
            <br />
            Better discovery.
          </h1>
          <p className="mt-3 text-sm text-muted">
            For artists who&apos;d rather be found than be sold to.
          </p>
        </header>

        <article className="space-y-6 text-[15px] leading-relaxed">
          <p>
            A Wander studio is your own page on the platform, with your
            work indexed alongside every other artist&apos;s. People
            search by what they&apos;re looking for — by image, by
            place, by sensibility — and your work surfaces when it
            matches.
          </p>

          <h2 className="font-serif text-2xl pt-4">What you get</h2>

          <ul className="list-disc list-outside ml-6 space-y-2">
            <li>
              A public studio page at{" "}
              <code className="font-mono text-sm">wander.gallery/artists/your-name</code>
            </li>
            <li>
              Your work appearing in search results and neighbourhood
              clusters across the platform
            </li>
            <li>
              A location pin on the map so people in your area can find
              you
            </li>
            <li>
              Inquiries forwarded straight to your inbox — we don&apos;t
              sit in the middle of the conversation
            </li>
          </ul>

          <h2 className="font-serif text-2xl pt-4">What we don&apos;t do</h2>

          <ul className="list-disc list-outside ml-6 space-y-2">
            <li>
              Take a commission on anything. We aren&apos;t a
              marketplace.
            </li>
            <li>Rank your work by how much it might sell for.</li>
            <li>
              Lock you into exclusivity. List your work wherever you
              like.
            </li>
            <li>
              Sell or share your details with anyone. See{" "}
              <Link href="/privacy" className="underline hover:text-foreground">
                privacy
              </Link>
              .
            </li>
          </ul>

          <h2 className="font-serif text-2xl pt-4">How it works</h2>

          <ol className="list-decimal list-outside ml-6 space-y-2">
            <li>
              Sign up — takes a minute, uses your email.
            </li>
            <li>
              Tell us about you and your work in a short onboarding
              flow.
            </li>
            <li>
              Upload your pieces, set whether each is available, add
              titles and (optional) prices.
            </li>
            <li>
              Drop in a studio location if you&apos;d like to be on the
              map.
            </li>
          </ol>

          <p className="pt-2">
            Onboarding is one-at-a-time right now while we get the
            platform shaped right. If anything feels off or missing,
            tell us at{" "}
            <a
              href="mailto:info@wander.gallery"
              className="underline hover:text-foreground"
            >
              info@wander.gallery
            </a>
            .
          </p>

          <div className="pt-8 flex flex-wrap gap-3">
            <Link
              href="/sign-up"
              className="inline-flex items-center px-5 py-2.5 bg-foreground text-background hover:bg-foreground/90 transition-colors"
            >
              Create your studio →
            </Link>
            <Link
              href="/about"
              className="inline-flex items-center px-5 py-2.5 border border-border bg-surface hover:bg-background transition-colors"
            >
              Read about Wander
            </Link>
          </div>
        </article>
      </main>
    </>
  );
}

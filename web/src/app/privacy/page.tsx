import Link from "next/link";
import type { Metadata } from "next";
import { TopNav } from "@/components/TopNav";

export const metadata: Metadata = {
  title: "Privacy",
  description:
    "What Wander collects, what we do with it, and what we don't do.",
};

export default function PrivacyPage() {
  return (
    <>
      <TopNav />
      <main className="flex-1 mx-auto w-full max-w-3xl px-6 py-12 md:py-16">
        <header className="mb-10">
          <h1 className="font-serif text-4xl md:text-5xl tracking-tight">
            Privacy
          </h1>
          <p className="mt-3 text-sm text-muted">
            What we collect and why. Plain English, not lawyer English.
          </p>
        </header>

        <article className="space-y-6 text-[15px] leading-relaxed">
          <p>
            Wander is a discovery site, not an ad platform. We collect
            the minimum we need to make search and inquiries work, and
            we don&apos;t share or sell any of it. The specifics:
          </p>

          <h2 className="font-serif text-2xl pt-4">If you visit without an account</h2>

          <p>
            We give your browser a signed anonymous cookie. It&apos;s
            an opaque identifier — no personal information, no third-
            party tracking attached. We use it to keep visual-search
            uploads tied to your session and to merge anonymous
            activity with your account if you sign up later.
          </p>

          <p>
            Our infrastructure logs request metadata — IP address,
            user-agent, requested URL — for security and abuse
            detection. Standard server-log retention.
          </p>

          <h2 className="font-serif text-2xl pt-4">If you sign up</h2>

          <p>
            We store your email, your studio name and bio, your work,
            optional location, and the messages people send you through
            the platform.
          </p>

          <h2 className="font-serif text-2xl pt-4">If you upload an image</h2>

          <p>
            Artwork images you publish are public. Visual-search
            uploads are not — they&apos;re tied to your session and
            expire after a short window.
          </p>

          <p>
            Collections (your saved lists of artworks) are private by
            default. If you choose to make one public, anyone with the
            share link can view it; the link itself is unguessable, but
            search engines may index it once it&apos;s shared anywhere
            public. Making a collection private again rotates the link
            so the old one stops working.
          </p>

          <h2 className="font-serif text-2xl pt-4">If you contact an artist</h2>

          <p>
            We forward your name, email, and message to the artist by
            email so they can reply directly. The message and a record
            of the exchange are stored on the platform.
          </p>

          <h2 className="font-serif text-2xl pt-4">Service providers</h2>

          <p>
            To run the platform we use third-party service providers
            for things like authentication, transactional email,
            hosting, map tiles, and error monitoring. They process
            data on our behalf under standard data-processing terms
            and only for the purpose of running the service. We
            don&apos;t share data with anyone for advertising or
            resale. If you&apos;d like the current list of
            sub-processors, email{" "}
            <a
              href="mailto:info@wander.gallery"
              className="underline hover:text-foreground"
            >
              info@wander.gallery
            </a>
            .
          </p>

          <h2 className="font-serif text-2xl pt-4">What we don&apos;t do</h2>

          <ul className="list-disc list-outside ml-6 space-y-2">
            <li>No third-party advertising. No tracking pixels.</li>
            <li>No selling or sharing your data with anyone.</li>
            <li>
              No behavioural profiles to push you toward what to buy.
            </li>
          </ul>

          <h2 className="font-serif text-2xl pt-4">Your rights</h2>

          <p>
            You can ask us to delete your account, your studio, your
            uploads, and all messages you&apos;ve sent or received at
            any time. Email{" "}
            <a
              href="mailto:info@wander.gallery"
              className="underline hover:text-foreground"
            >
              info@wander.gallery
            </a>{" "}
            and we&apos;ll do it within seven days. You can also ask
            for a copy of everything we hold about you.
          </p>

          <h2 className="font-serif text-2xl pt-4">Changes</h2>

          <p>
            If we change anything material, we&apos;ll email signed-up
            users before it takes effect. This page is the source of
            truth for current behaviour.
          </p>

          <p className="pt-4 text-sm text-muted">
            Questions? Email{" "}
            <a
              href="mailto:info@wander.gallery"
              className="underline hover:text-foreground"
            >
              info@wander.gallery
            </a>
            . See also{" "}
            <Link href="/terms" className="underline hover:text-foreground">
              terms of use
            </Link>
            .
          </p>
        </article>
      </main>
    </>
  );
}

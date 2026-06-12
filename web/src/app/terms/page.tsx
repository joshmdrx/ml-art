import Link from "next/link";
import type { Metadata } from "next";
import { TopNav } from "@/components/TopNav";

export const metadata: Metadata = {
  title: "Terms — Wander",
  description: "The rules of using Wander, in plain English.",
};

export default function TermsPage() {
  return (
    <>
      <TopNav />
      <main className="flex-1 mx-auto w-full max-w-3xl px-6 py-12 md:py-16">
        <header className="mb-10">
          <h1 className="font-serif text-4xl md:text-5xl tracking-tight">
            Terms
          </h1>
          <p className="mt-3 text-sm text-muted">
            The short version: behave; you keep your work; we&apos;ll
            be reasonable.
          </p>
        </header>

        <article className="space-y-6 text-[15px] leading-relaxed">
          <h2 className="font-serif text-2xl">Your work stays yours</h2>

          <p>
            Anything you upload to Wander — images, descriptions,
            prices, studio details — is yours. We don&apos;t claim
            ownership and we don&apos;t take a cut of anything that
            happens after a buyer finds you.
          </p>

          <p>
            You grant us permission to host, resize, and display your
            work on the platform (we wouldn&apos;t be much use as a
            discovery site otherwise). That permission ends the moment
            you delete the work or your account.
          </p>

          <h2 className="font-serif text-2xl pt-4">Don&apos;t upload things you don&apos;t own</h2>

          <p>
            By uploading work, you confirm you made it or have the
            right to publish it. If you upload someone else&apos;s work
            without permission and we get a credible complaint, we
            remove it.
          </p>

          <h2 className="font-serif text-2xl pt-4">Be decent</h2>

          <p>
            Don&apos;t use Wander to harass anyone, send spam, send
            unsolicited commercial messages through the inquiry system,
            or upload anything illegal. We&apos;ll remove accounts that
            do.
          </p>

          <p>
            Don&apos;t attempt to scrape the platform at scale, bypass
            the rate-limits, or otherwise mess with how it works. The
            search and image data is free for humans to enjoy; bulk
            scraping is not.
          </p>

          <h2 className="font-serif text-2xl pt-4">Moderation</h2>

          <p>
            We screen uploaded images for things like explicit content
            and visible illegal material. If something gets flagged
            incorrectly,{" "}
            <a
              href="mailto:info@wander.gallery"
              className="underline hover:text-foreground"
            >
              email us
            </a>{" "}
            and we&apos;ll review by hand.
          </p>

          <h2 className="font-serif text-2xl pt-4">No warranty</h2>

          <p>
            Wander is provided as-is. The site might be slow or
            unavailable. Search results might be weird. We&apos;ll do
            our best to keep things working, but we&apos;re not
            promising uptime or perfect results, and we&apos;re not
            liable for losses from outages or bugs.
          </p>

          <h2 className="font-serif text-2xl pt-4">Disputes</h2>

          <p>
            If we ever have a serious disagreement, English law applies
            and the English courts have jurisdiction.
          </p>

          <h2 className="font-serif text-2xl pt-4">Changes</h2>

          <p>
            If we change anything material in these terms, signed-up
            users will get an email before it takes effect. This page is
            the source of truth for what&apos;s currently in force.
          </p>

          <p className="pt-4 text-sm text-muted">
            See also{" "}
            <Link href="/privacy" className="underline hover:text-foreground">
              privacy
            </Link>
            . Questions:{" "}
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

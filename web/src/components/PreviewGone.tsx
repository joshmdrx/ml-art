import Link from "next/link";
import { TopNav } from "@/components/TopNav";

/** T-086 — what a dead `/preview/<token>` link (or one of its artwork
 * pages) shows: expired, replaced, claimed, removed or never valid all
 * look the same. */
export function PreviewGone() {
  return (
    <>
      <TopNav />
      <main className="flex-1 mx-auto w-full max-w-2xl px-6 py-20">
        <h1 className="font-serif text-3xl tracking-tight">
          This preview link no longer works
        </h1>
        <p className="mt-4 text-muted">
          It may have expired, been replaced with a newer link, been
          claimed, or been removed. If you were expecting to see a
          draft page, reply to the email it came in and we&apos;ll
          sort it out.
        </p>
        <p className="mt-8 text-sm">
          <Link href="/" className="underline hover:no-underline">
            Go to Wander
          </Link>
        </p>
      </main>
    </>
  );
}

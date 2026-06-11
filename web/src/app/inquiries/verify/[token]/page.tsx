import type { Metadata } from "next";
import Link from "next/link";
import { TopNav } from "@/components/TopNav";
import { verifyInquiry } from "@/lib/api";
import { reportError } from "@/lib/reportError";

/**
 * /inquiries/verify/[token] — landing page for the link in the
 * anonymous-inquiry confirmation email. Marks the inquiry verified +
 * delivered server-side, then renders a success / not-found message.
 *
 * Single-use by design — but visiting twice is safe (the API is
 * idempotent: the UPDATE only flips NULL → now() on the timestamps).
 */

export const metadata: Metadata = {
  title: "Inquiry confirmed — Wander",
};

type Params = Promise<{ token: string }>;

export default async function VerifyPage({ params }: { params: Params }) {
  const { token } = await params;

  let result: { ok: true } | { ok: false; reason: "not-found" | "error" };
  try {
    const r = await verifyInquiry(token);
    result = r ? { ok: true } : { ok: false, reason: "not-found" };
  } catch (e) {
    reportError(e, { surface: "verify-inquiry", token });
    result = { ok: false, reason: "error" };
  }

  return (
    <>
      <TopNav />
      <main className="flex-1 mx-auto w-full max-w-xl px-6 py-24">
        {result.ok ? (
          <>
            <h1 className="font-serif text-4xl">Sent.</h1>
            <p className="mt-4 text-sm text-muted">
              Your inquiry is on its way to the artist. They&apos;ll reply to
              the email you used.
            </p>
            <p className="mt-12 text-xs text-muted">
              <Link href="/" className="hover:text-foreground">
                ← Back to home
              </Link>
            </p>
          </>
        ) : (
          <>
            <h1 className="font-serif text-4xl">Link doesn&apos;t look right.</h1>
            <p className="mt-4 text-sm text-muted">
              {result.reason === "not-found"
                ? "We can't find an inquiry matching this link. It may have expired or already been used."
                : "Something went wrong on our side. Try the link again in a moment."}
            </p>
            <p className="mt-12 text-xs text-muted">
              <Link href="/" className="hover:text-foreground">
                ← Back to home
              </Link>
            </p>
          </>
        )}
      </main>
    </>
  );
}

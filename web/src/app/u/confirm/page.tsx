import type { Metadata } from "next";
import Link from "next/link";
import { unsubscribeWithToken } from "@/lib/api";
import { TopNav } from "@/components/TopNav";
import { reportError, toUserMessage } from "@/lib/reportError";

/**
 * T-068 — confirmation landing for the GET unsubscribe flow.
 *
 * `/u/[token]/route.ts` GET redirects here with `?token=<jwt>`. We do
 * the actual unsubscribe on this server render, then show what happened.
 * Idempotent — refreshing the page issues the same flip-to-disabled
 * call, which is a no-op the second time.
 */

export const metadata: Metadata = {
  title: "Unsubscribed",
};

interface Search {
  token?: string;
}

export default async function UnsubscribeConfirmPage({
  searchParams,
}: {
  searchParams: Promise<Search>;
}) {
  const sp = await searchParams;
  const token = sp.token;

  let kindLabel: string | null = null;
  let errorMessage: string | null = null;

  if (!token) {
    errorMessage = "Unsubscribe link missing its token.";
  } else {
    try {
      const ack = await unsubscribeWithToken(token);
      kindLabel = ack.friendly_label || ack.kind;
    } catch (e) {
      errorMessage = toUserMessage(
        e,
        "This unsubscribe link isn't valid or has expired. You can manage all your email preferences from settings instead.",
        { surface: "unsubscribe-confirm" },
      );
      reportError(e, { surface: "unsubscribe-confirm" });
    }
  }

  return (
    <>
      <TopNav />
      <main className="flex-1 mx-auto w-full max-w-2xl px-6 py-16 md:py-24">
        {kindLabel ? (
          <>
            <h1 className="font-serif text-3xl md:text-4xl tracking-tight">
              You&apos;re unsubscribed.
            </h1>
            <p className="mt-4 text-base leading-relaxed">
              You won&apos;t receive any more <strong>{kindLabel}</strong>{" "}
              emails from Wander. Transactional emails (verifying an
              inquiry you sent, an artist&apos;s reply to your message)
              will still go through — they&apos;re part of how the
              product works.
            </p>
            <p className="mt-6 text-sm text-muted">
              Changed your mind? You can switch this back on, or fine-tune
              any other preferences, in{" "}
              <Link
                href="/me/settings/notifications"
                className="underline hover:text-foreground"
              >
                your email-notification settings
              </Link>
              .
            </p>
          </>
        ) : (
          <>
            <h1 className="font-serif text-3xl md:text-4xl tracking-tight">
              Hmm.
            </h1>
            <p className="mt-4 text-base leading-relaxed">{errorMessage}</p>
            <p className="mt-6 text-sm text-muted">
              <Link
                href="/me/settings/notifications"
                className="underline hover:text-foreground"
              >
                Manage email notifications
              </Link>
            </p>
          </>
        )}
      </main>
    </>
  );
}

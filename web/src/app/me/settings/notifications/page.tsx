import type { Metadata } from "next";
import { redirect } from "next/navigation";
import Link from "next/link";
import { auth } from "@clerk/nextjs/server";
import { TopNav } from "@/components/TopNav";
import { NotificationSettingsForm } from "@/components/NotificationSettingsForm";
import { getNotificationPreferences } from "@/lib/api";
import { reportError, toUserMessage } from "@/lib/reportError";

/**
 * T-068 — manage all notification preferences. The client form
 * handles toggles via a server action; this server component loads
 * initial state.
 */

export const metadata: Metadata = {
  title: "Email notifications",
};

// Friendly metadata per kind, kept on the web side so we can localise
// later without redeploying the API. The wire-format kind names come
// from `core::notifications::NotificationKind`.
const KIND_META: Record<string, { label: string; description: string }> = {
  new_works_digest: {
    label: "New work from artists you follow",
    description:
      "A daily summary, only sent on days when at least one artist you follow publishes new work.",
  },
};

export default async function NotificationSettingsPage() {
  const { userId } = await auth();
  if (!userId) {
    redirect(
      "/sign-in?redirect_url=" +
        encodeURIComponent("/me/settings/notifications"),
    );
  }

  let initial;
  let loadError: string | null = null;
  try {
    initial = await getNotificationPreferences();
  } catch (e) {
    loadError = toUserMessage(
      e,
      "Couldn't load your notification preferences. Try again in a moment.",
      { surface: "notification-settings", call: "get" },
    );
    reportError(e, { surface: "notification-settings", call: "get" });
  }

  return (
    <>
      <TopNav />
      <main className="flex-1 mx-auto w-full max-w-2xl px-6 py-12 md:py-16">
        <p className="text-xs text-muted mb-3">
          <Link href="/me/settings" className="hover:text-foreground">
            ← Settings
          </Link>
        </p>
        <header className="mb-10">
          <h1 className="font-serif text-3xl md:text-4xl tracking-tight">
            Email notifications
          </h1>
          <p className="mt-3 text-sm text-muted">
            We never share your email. Transactional emails (inquiry
            verification, replies to inquiries you sent) always go through —
            they&apos;re part of how the product works.
          </p>
        </header>

        {loadError ? (
          <p className="text-sm">{loadError}</p>
        ) : (
          <NotificationSettingsForm initial={initial!} kindMeta={KIND_META} />
        )}
      </main>
    </>
  );
}

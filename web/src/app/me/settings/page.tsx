import type { Metadata } from "next";
import { redirect } from "next/navigation";
import Link from "next/link";
import { auth } from "@clerk/nextjs/server";
import { TopNav } from "@/components/TopNav";

/**
 * T-068 — `/me/settings` index. Lightweight hub page so future
 * settings sections (account, privacy, data export) have a clear
 * home. Today only Notifications lives here.
 */

export const metadata: Metadata = {
  title: "Settings",
};

export default async function SettingsIndex() {
  const { userId } = await auth();
  if (!userId) {
    redirect("/sign-in?redirect_url=" + encodeURIComponent("/me/settings"));
  }

  return (
    <>
      <TopNav />
      <main className="flex-1 mx-auto w-full max-w-3xl px-6 py-12 md:py-16">
        <header className="mb-10">
          <h1 className="font-serif text-4xl md:text-5xl tracking-tight">
            Settings
          </h1>
        </header>

        <ul className="divide-y divide-border border-y border-border">
          <li>
            <Link
              href="/me/settings/notifications"
              className="flex items-center justify-between py-5 hover:bg-surface transition-colors px-4 -mx-4"
            >
              <div>
                <p className="font-medium">Email notifications</p>
                <p className="mt-1 text-sm text-muted">
                  Choose which emails you receive — new work from artists
                  you follow, and others.
                </p>
              </div>
              <span aria-hidden className="text-muted">
                →
              </span>
            </Link>
          </li>
        </ul>

        <p className="mt-12 text-xs text-muted">
          Account settings (email, password, sign-in methods) are managed
          via your{" "}
          <Link href="/" className="underline hover:text-foreground">
            account avatar
          </Link>
          .
        </p>
      </main>
    </>
  );
}

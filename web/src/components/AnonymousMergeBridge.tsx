"use client";

/**
 * T-033 — fire-and-forget client bridge that asks the server to merge
 * the anon_id cookie's behavioral signal into the now-known user.
 *
 * Why client-side: server components can read `cookies()` but can't set
 * them. We use `sessionStorage` as the "already-merged this session"
 * marker so we don't pile up redundant API calls on every navigation.
 *
 * Failures are silent — the merge is best-effort. A second sign-in
 * session will re-attempt (sessionStorage resets), and the underlying
 * API call is idempotent on the server side.
 */

import { useEffect } from "react";
import { useAuth } from "@clerk/nextjs";
import { reportError } from "@/lib/reportError";

const SESSION_KEY = "mlart_anon_merged";

export function AnonymousMergeBridge() {
  const { isLoaded, isSignedIn } = useAuth();

  useEffect(() => {
    if (!isLoaded || !isSignedIn) return;
    // SSR guard — sessionStorage doesn't exist server-side. The
    // `useEffect` already runs only on the client, but the lint rule
    // is happier with the explicit check.
    if (typeof window === "undefined") return;
    if (window.sessionStorage.getItem(SESSION_KEY) === "1") return;

    // Mark up-front so a fast double-navigation can't fire twice. If
    // the call fails we'll log but won't retry within the same
    // session — the API is idempotent and the next session will pick
    // it up.
    window.sessionStorage.setItem(SESSION_KEY, "1");

    fetch("/api/me/merge-anonymous", { method: "POST" })
      .then((res) => {
        if (!res.ok) {
          // Re-arm so a subsequent navigation gets another shot.
          window.sessionStorage.removeItem(SESSION_KEY);
          throw new Error(`merge-anonymous bridge ${res.status}`);
        }
      })
      .catch((e) => {
        reportError(e, { surface: "anonymous-merge-bridge" });
      });
  }, [isLoaded, isSignedIn]);

  return null;
}

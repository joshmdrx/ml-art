"use client";

/**
 * "← Back to search" link that respects the user's search state.
 *
 * Why a client component for what looks like a static link:
 *
 *   - The search page's "where I was" state (pages, focus, scroll,
 *     map viewport) lives in the URL plus browser-managed scroll
 *     position. A fresh `<Link href="/search">` strips all of it.
 *   - `router.back()` reuses the browser history entry — same URL,
 *     same scroll, same focus restore — but only works when the
 *     previous page actually was /search. For deep-links or
 *     multi-hop traversal we'd otherwise back into something
 *     unrelated (google, /collections, etc.), which is the wrong
 *     mental model for a button labelled "Back to search".
 *
 * Behaviour:
 *
 *   - If `document.referrer` is our own /search → `router.back()`
 *     (full state restore).
 *   - Otherwise → `router.push('/search')` (matches what the label
 *     promises).
 *   - The underlying `<a href="/search">` keeps middle-click + cmd-
 *     click + right-click "Open in new tab" working, and is what
 *     non-JS / crawler reads.
 */

import { useRouter } from "next/navigation";

interface Props {
  /** Link content. Defaults to "← Back to search". */
  children?: React.ReactNode;
  className?: string;
}

export function BackToSearchLink({
  children = "← Back to search",
  className,
}: Props) {
  const router = useRouter();

  function onClick(e: React.MouseEvent<HTMLAnchorElement>) {
    // Don't intercept new-tab / new-window clicks — let the browser
    // do its native thing.
    if (
      e.defaultPrevented ||
      e.metaKey ||
      e.ctrlKey ||
      e.shiftKey ||
      e.altKey ||
      e.button !== 0
    ) {
      return;
    }
    if (cameFromSearch()) {
      e.preventDefault();
      router.back();
    }
    // Else fall through — the <a href="/search"> handles it.
  }

  return (
    <a href="/search" onClick={onClick} className={className}>
      {children}
    </a>
  );
}

/**
 * Best-effort check that the previous page in this tab's history
 * is our `/search`. Same-origin referrer protects against cross-
 * site users with a stale referrer policy; pathname check guards
 * against the user e.g. coming from `/collections`.
 */
function cameFromSearch(): boolean {
  if (typeof document === "undefined") return false;
  const ref = document.referrer;
  if (!ref) return false;
  try {
    const u = new URL(ref);
    if (u.origin !== window.location.origin) return false;
    return u.pathname === "/search";
  } catch {
    return false;
  }
}

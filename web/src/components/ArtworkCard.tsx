"use client";

import { useCallback, useRef, useState } from "react";
import Link from "next/link";
// Type-only import is erased at compile time, so `lib/api`'s
// runtime Clerk import doesn't follow into the client bundle.
import type { ArtworkSummary, Paginated } from "@/lib/api";
// Pure formatter, no server-only deps — safe in client components.
import { formatPrice } from "@/lib/format";
import { reportError } from "@/lib/reportError";

/**
 * Card used in every grid view.
 *
 * The image links to artwork detail; the artist name is a *separate* link to
 * the artist portfolio. Can't nest <a> inside <a>, so they're siblings.
 *
 * T-063 — inline "more like this" tray. Hovering the card for ≥600ms
 * lazy-fetches `/v1/artworks/:id/similar` and reveals a 4-thumb strip
 * below the card. The fetch is single-flight per card (cached after
 * first hover); the tray hides on mouse-leave but the data is kept so
 * a re-hover is instant. A small "···" button surfaces the same
 * affordance on touch devices where hover-intent doesn't exist.
 */

const HOVER_DELAY_MS = 600;
const TRAY_LIMIT = 4;

export function ArtworkCard({ artwork }: { artwork: ArtworkSummary }) {
  const price = formatPrice(artwork.price_cents, artwork.currency);
  const altText = artwork.title
    ? `${artwork.title} by ${artwork.artist_name}`
    : `Untitled by ${artwork.artist_name}`;

  const [trayOpen, setTrayOpen] = useState(false);
  const [similar, setSimilar] = useState<ArtworkSummary[] | null>(null);
  const [fetchState, setFetchState] = useState<"idle" | "loading" | "error">(
    "idle",
  );
  const hoverTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);

  const fetchSimilar = useCallback(async () => {
    if (similar !== null || fetchState === "loading") return;
    setFetchState("loading");
    try {
      const res = await fetch(
        `/api/artworks/${encodeURIComponent(artwork.id)}/similar?limit=${TRAY_LIMIT}`,
      );
      if (!res.ok) throw new Error(`similar ${res.status}`);
      const body = (await res.json()) as Paginated<ArtworkSummary>;
      // Filter out the card's own artwork if the API returned it
      // (shouldn't but defensive).
      setSimilar(body.items.filter((a) => a.id !== artwork.id));
      setFetchState("idle");
    } catch (e) {
      reportError(e, { surface: "artwork-card-similar", artwork_id: artwork.id });
      setFetchState("error");
    }
  }, [artwork.id, similar, fetchState]);

  const onMouseEnter = useCallback(() => {
    if (hoverTimerRef.current) clearTimeout(hoverTimerRef.current);
    hoverTimerRef.current = setTimeout(() => {
      setTrayOpen(true);
      fetchSimilar();
    }, HOVER_DELAY_MS);
  }, [fetchSimilar]);

  const onMouseLeave = useCallback(() => {
    if (hoverTimerRef.current) {
      clearTimeout(hoverTimerRef.current);
      hoverTimerRef.current = null;
    }
    setTrayOpen(false);
  }, []);

  const onTrayToggle = useCallback(() => {
    setTrayOpen((prev) => {
      const next = !prev;
      if (next) fetchSimilar();
      return next;
    });
  }, [fetchSimilar]);

  return (
    <div
      className="group relative"
      onMouseEnter={onMouseEnter}
      onMouseLeave={onMouseLeave}
    >
      <Link
        href={`/artworks/${artwork.id}`}
        aria-label={altText}
        className="block bg-surface border border-border overflow-hidden"
      >
        {artwork.primary_image_url ? (
          // Plain <img> — Next/Image needs a remote-host allow-list, will
          // configure once we lock in CloudFront.
          // eslint-disable-next-line @next/next/no-img-element
          <img
            src={artwork.primary_image_url}
            alt={altText}
            loading="lazy"
            className="w-full h-auto block transition-opacity group-hover:opacity-95"
          />
        ) : (
          <div className="aspect-square bg-border" />
        )}
      </Link>

      <div className="mt-2 px-0.5 flex items-start justify-between gap-2">
        <div className="min-w-0 flex-1">
          <Link
            href={`/artists/${artwork.artist_slug}`}
            className="text-sm text-muted hover:text-foreground truncate block"
          >
            {artwork.artist_name}
          </Link>
          <Link
            href={`/artworks/${artwork.id}`}
            className="text-sm font-serif hover:underline truncate block"
          >
            {artwork.title ?? "Untitled"}
          </Link>
          {price && <div className="text-xs text-muted mt-0.5">{price}</div>}
        </div>
        {/* Explicit affordance for touch devices where hover-intent
            doesn't exist. Subtle on desktop — also a fallback if the
            hover delay doesn't fire (e.g. accessibility setting). */}
        <button
          type="button"
          onClick={onTrayToggle}
          aria-label={
            trayOpen ? "Hide similar artworks" : "Show similar artworks"
          }
          aria-expanded={trayOpen}
          className="shrink-0 text-muted hover:text-foreground text-base leading-none p-1 -m-1 focus:outline-none focus-visible:ring-2 focus-visible:ring-foreground"
        >
          ···
        </button>
      </div>

      {trayOpen && (
        <SimilarTray
          state={fetchState}
          items={similar}
          // Aria for screen readers — the tray expands the card's
          // visible content, not a separate region.
          ariaLabel={`More like ${artwork.title ?? "this artwork"}`}
        />
      )}
    </div>
  );
}

function SimilarTray({
  state,
  items,
  ariaLabel,
}: {
  state: "idle" | "loading" | "error";
  items: ArtworkSummary[] | null;
  ariaLabel: string;
}) {
  return (
    <div
      aria-label={ariaLabel}
      className="mt-2 border-t border-border pt-2"
    >
      {state === "loading" && (
        <p className="text-xs text-muted px-0.5">Looking for similar…</p>
      )}
      {state === "error" && (
        <p className="text-xs text-muted px-0.5">
          Couldn&apos;t load similar works.
        </p>
      )}
      {state === "idle" && items && items.length === 0 && (
        <p className="text-xs text-muted px-0.5">
          No similar works yet.
        </p>
      )}
      {state === "idle" && items && items.length > 0 && (
        <ul className="grid grid-cols-4 gap-1">
          {items.slice(0, TRAY_LIMIT).map((a) => (
            <li key={a.id}>
              <Link
                href={`/artworks/${a.id}`}
                className="block aspect-square overflow-hidden bg-background border border-border hover:opacity-90"
                title={
                  a.title
                    ? `${a.title} — ${a.artist_name}`
                    : `Untitled — ${a.artist_name}`
                }
              >
                {a.primary_image_url ? (
                  // eslint-disable-next-line @next/next/no-img-element
                  <img
                    src={a.primary_image_url}
                    alt={a.title ?? "Untitled"}
                    loading="lazy"
                    className="w-full h-full object-cover"
                  />
                ) : (
                  <div className="w-full h-full bg-border" />
                )}
              </Link>
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}

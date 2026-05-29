"use client";

/**
 * T-043 — "Near me" affordance for the map.
 *
 * Asks the browser for the user's coordinates via `navigator.geolocation`
 * (permission prompt the first time), then navigates to `/search?map=1`
 * with a `bbox` centered on those coords. The map's existing
 * `SearchMap` component reads `bbox` from the URL and `fitBounds` to
 * it on mount, so this is purely a URL-mutation affordance.
 *
 * Two render contexts:
 *   - `variant="hero"` — bigger pill for the homepage hero
 *   - `variant="inline"` — small button next to the Grid/Map toggle
 *
 * If geolocation isn't available (server-side render, opted-out
 * browser, embedded webview, etc.) we hide the button entirely
 * rather than render a non-functional control. That's safer than
 * surfacing "this won't work" — most users don't know the difference
 * between "denied permission" and "browser doesn't support."
 */

import { useEffect, useState, useTransition } from "react";
import { useRouter } from "next/navigation";
import { reportError } from "@/lib/reportError";

interface Props {
  variant?: "hero" | "inline";
}

/** Radius around the user's coords, in degrees. ~0.05° ≈ 5km at
 * temperate latitudes — wide enough to catch a few galleries, narrow
 * enough that the map zooms in usefully rather than showing a
 * country-sized box. */
const RADIUS_DEG = 0.05;

export function NearMeButton({ variant = "inline" }: Props) {
  const router = useRouter();
  const [available, setAvailable] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [isPending, startTransition] = useTransition();

  // Geolocation API availability is a window-only check; SSR sees
  // `window === undefined`. Render-then-detect avoids a hydration
  // mismatch — same pattern + lint exception we use in `SaveModal`
  // and `ArtworkEditModal` for legitimate "sync from external system
  // on mount" reads.
  useEffect(() => {
    // eslint-disable-next-line react-hooks/set-state-in-effect
    setAvailable(
      typeof window !== "undefined" && "geolocation" in window.navigator
    );
  }, []);

  if (!available) return null;

  function onClick() {
    setError(null);
    startTransition(() => {
      navigator.geolocation.getCurrentPosition(
        (pos) => {
          const { latitude: lat, longitude: lng } = pos.coords;
          const west = lng - RADIUS_DEG;
          const east = lng + RADIUS_DEG;
          const south = lat - RADIUS_DEG;
          const north = lat + RADIUS_DEG;
          const bbox = [west, south, east, north]
            .map((n) => n.toFixed(4))
            .join(",");
          router.push(`/search?map=1&bbox=${bbox}`);
        },
        (geoErr) => {
          // PERMISSION_DENIED (1) is the common case — don't log it
          // as an error, just surface a soft message.
          if (geoErr.code === geoErr.PERMISSION_DENIED) {
            setError("Allow location access to use this feature.");
          } else {
            reportError(geoErr, { surface: "near-me-button" });
            setError("Couldn't get your location.");
          }
        },
        {
          // 10s is long but geolocation on cold-start (GPS warmup) can
          // genuinely take that long on mobile. Better than failing fast.
          timeout: 10_000,
          maximumAge: 60_000,
        }
      );
    });
  }

  const heroClass =
    "inline-flex items-center gap-2 border border-border bg-surface hover:bg-fg/10 px-4 py-2 text-sm";
  const inlineClass =
    "inline-flex items-center gap-1.5 border border-border bg-bg hover:bg-surface px-3 py-1.5 text-sm";
  const className = variant === "hero" ? heroClass : inlineClass;

  return (
    <div className="inline-flex flex-col gap-1">
      <button
        type="button"
        onClick={onClick}
        disabled={isPending}
        className={`${className} disabled:opacity-50`}
      >
        <PinIcon />
        {isPending ? "Locating…" : "Near me"}
      </button>
      {error && (
        <p className="text-xs text-amber-700" role="status">
          {error}
        </p>
      )}
    </div>
  );
}

function PinIcon() {
  return (
    <svg
      aria-hidden="true"
      width="14"
      height="14"
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth="2"
      strokeLinecap="round"
      strokeLinejoin="round"
    >
      <path d="M21 10c0 7-9 13-9 13s-9-6-9-13a9 9 0 1 1 18 0z" />
      <circle cx="12" cy="10" r="3" />
    </svg>
  );
}

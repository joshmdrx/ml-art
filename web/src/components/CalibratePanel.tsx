"use client";

/**
 * T-061 — first-session taste calibrator panel.
 *
 * Renders a 5-pair "this or that" interaction on the homepage to seed
 * the user's taste vector before they've had a chance to save or
 * inquire. Picks are sent to `POST /api/calibrate/pick` which bridges
 * to the api; the api emits a `calibration_pick` event (weight 2.0)
 * that T-055's refresh picks up.
 *
 * Visibility rules:
 *   - Hidden if `localStorage["wander:calibrator"]` is set ("done" or
 *     "skip" — either way the user has had their say).
 *   - Hidden if the SSR'd `pairs` prop is empty (corpus has fewer
 *     than 2 semantic neighbourhoods).
 *   - Hidden after the user completes or skips during the session.
 *
 * Once dismissed, never auto-resurfaces. Curious users can re-trigger
 * it by clearing the localStorage entry.
 */

import { useEffect, useState } from "react";

import type { CalibratePair } from "@/lib/api";
import { reportError } from "@/lib/reportError";

const STORAGE_KEY = "wander:calibrator";

type DoneState = "active" | "completed" | "skipped";

export function CalibratePanel({ pairs }: { pairs: CalibratePair[] }) {
  // SSR-safe: render nothing until we know localStorage state. Avoids
  // a flash-of-panel for returning visitors who've already dismissed.
  const [hydrated, setHydrated] = useState(false);
  const [shouldShow, setShouldShow] = useState(false);
  const [currentIdx, setCurrentIdx] = useState(0);
  const [state, setState] = useState<DoneState>("active");

  useEffect(() => {
    // Reading localStorage isn't possible during SSR, so we flip
    // `hydrated` + `shouldShow` from inside the effect on first mount.
    // The shouldShow set below is the load-from-external-store
    // handshake (same pattern SaveModal uses for its on-open
    // transition); the rule flags it but it's the intent.
    // eslint-disable-next-line react-hooks/set-state-in-effect
    setHydrated(true);
    if (pairs.length < 1) return;
    let flag: string | null = null;
    try {
      flag = window.localStorage.getItem(STORAGE_KEY);
    } catch {
      // localStorage blocked (Safari private mode, etc.) — show
      // anyway. The component just won't remember between sessions.
    }
    if (flag === "done" || flag === "skip") return;
    // eslint-disable-next-line react-hooks/set-state-in-effect
    setShouldShow(true);
  }, [pairs.length]);

  if (!hydrated || !shouldShow || pairs.length === 0) {
    return null;
  }

  // Defensive guard: completion state can lag the index by one frame
  // when the last pick fires. Avoid an out-of-bounds while we wait.
  if (currentIdx >= pairs.length && state === "active") {
    return null;
  }

  if (state === "completed" || state === "skipped") {
    return (
      <CalibrateDoneBanner
        completed={state === "completed"}
        onClose={() => setShouldShow(false)}
      />
    );
  }

  const pair = pairs[currentIdx];

  const sendPick = async (chosenSide: "left" | "right") => {
    const chosen = chosenSide === "left" ? pair.left : pair.right;
    const rejected = chosenSide === "left" ? pair.right : pair.left;
    try {
      await fetch("/api/calibrate/pick", {
        method: "POST",
        headers: { "content-type": "application/json" },
        body: JSON.stringify({
          pair_id: pair.id,
          chosen_artwork_id: chosen.artwork_id,
          rejected_artwork_id: rejected.artwork_id,
        }),
        keepalive: true,
      });
    } catch (e) {
      // Don't block the UI on a network blip — the next refresh will
      // still pick up the prior picks. Log so we notice systemic issues.
      reportError(e, { surface: "calibrate", pair_id: pair.id });
    }
    if (currentIdx + 1 >= pairs.length) {
      try {
        window.localStorage.setItem(STORAGE_KEY, "done");
      } catch {
        /* ignore */
      }
      setState("completed");
    } else {
      setCurrentIdx((i) => i + 1);
    }
  };

  const skip = () => {
    try {
      window.localStorage.setItem(STORAGE_KEY, "skip");
    } catch {
      /* ignore */
    }
    setState("skipped");
  };

  return (
    <section
      aria-label="Help us tune what to show you"
      className="mx-auto max-w-screen-2xl px-6 pb-12"
    >
      <div className="border border-border bg-surface p-6 md:p-8">
        <div className="flex items-baseline justify-between mb-2">
          <h2 className="font-serif text-xl md:text-2xl">
            Help us tune what to show you
          </h2>
          <button
            type="button"
            onClick={skip}
            className="text-sm text-muted hover:text-foreground"
          >
            Skip
          </button>
        </div>
        <p className="text-sm text-muted mb-6">
          Tap the one that pulls you in. {currentIdx + 1} of {pairs.length}.
        </p>

        <div className="grid grid-cols-2 gap-3 md:gap-6">
          <CalibrateCard
            side="left"
            artwork={pair.left}
            onPick={() => sendPick("left")}
          />
          <CalibrateCard
            side="right"
            artwork={pair.right}
            onPick={() => sendPick("right")}
          />
        </div>
      </div>
    </section>
  );
}

function CalibrateCard({
  side,
  artwork,
  onPick,
}: {
  side: "left" | "right";
  artwork: CalibratePair["left"];
  onPick: () => void;
}) {
  return (
    <button
      type="button"
      onClick={onPick}
      aria-label={`Choose ${side}: ${artwork.title ?? "untitled"} by ${artwork.artist_name}`}
      className="group flex flex-col text-left hover:opacity-95 focus:outline-none focus-visible:ring-2 focus-visible:ring-foreground"
    >
      <div className="relative aspect-square w-full overflow-hidden bg-surface-2">
        {/* Plain <img> — next/image needs a remote-host allowlist;
            follow the ArtworkCard convention until that's wired up. */}
        {/* eslint-disable-next-line @next/next/no-img-element */}
        <img
          src={artwork.image_url}
          alt={artwork.title ?? "Untitled"}
          loading="lazy"
          className="w-full h-full object-cover transition-transform group-hover:scale-[1.02]"
        />
      </div>
      <div className="pt-2 text-sm">
        <div className="line-clamp-1">{artwork.title ?? "Untitled"}</div>
        <div className="text-muted text-xs line-clamp-1">{artwork.artist_name}</div>
      </div>
    </button>
  );
}

function CalibrateDoneBanner({
  completed,
  onClose,
}: {
  completed: boolean;
  onClose: () => void;
}) {
  return (
    <section className="mx-auto max-w-screen-2xl px-6 pb-12">
      <div className="border border-border bg-surface p-4 md:p-6 flex items-center justify-between">
        <p className="text-sm">
          {completed
            ? "Thanks — we'll use that to start tuning your feed."
            : "Got it — we'll skip the calibration."}
        </p>
        <button
          type="button"
          onClick={onClose}
          className="text-sm text-muted hover:text-foreground"
        >
          Dismiss
        </button>
      </div>
    </section>
  );
}

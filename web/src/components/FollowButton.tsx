"use client";

import { useState, useTransition } from "react";
import { useRouter } from "next/navigation";
import { useAuth } from "@clerk/nextjs";
import {
  followArtistAction,
  queueAnonFollowAction,
  unfollowArtistAction,
} from "@/app/actions/follows";
import { reportError } from "@/lib/reportError";

/**
 * T-052 — follow/unfollow control rendered on `/artists/[slug]`.
 *
 * Signed-out: queue the follow intent against the anon_id cookie
 * (T-052c) so the merge-anonymous handler replays it after sign-in,
 * then redirect to sign-in with `redirect_url=` set to the current
 * artist page. The intent capture is best-effort — failures don't
 * block the redirect.
 *
 * Signed-in: optimistic update (flip state immediately, let the
 * server action settle); on error, revert.
 */
export function FollowButton({
  artistId,
  artistSlug,
  initialIsFollowing,
  className,
}: {
  artistId: string;
  artistSlug: string;
  initialIsFollowing: boolean;
  className?: string;
}) {
  const { isSignedIn, isLoaded } = useAuth();
  const router = useRouter();
  const [isFollowing, setIsFollowing] = useState(initialIsFollowing);
  const [pending, startTransition] = useTransition();

  function handleClick() {
    if (!isLoaded) return;

    if (!isSignedIn) {
      // T-052c — capture the intent so it replays after sign-in.
      // Best-effort: don't block the redirect on a queue failure
      // (the user can always click Follow again after signing in).
      // Use a transition so the redirect doesn't fire until the
      // queue call has at least had a tick.
      startTransition(async () => {
        try {
          await queueAnonFollowAction(artistId);
        } catch (e) {
          reportError(e, {
            surface: "follow-button-queue-anon",
            artistId,
          });
        }
        const redirect = encodeURIComponent(`/artists/${artistSlug}`);
        router.push(`/sign-in?redirect_url=${redirect}`);
      });
      return;
    }

    // Optimistic flip; revert on error.
    const next = !isFollowing;
    setIsFollowing(next);
    startTransition(async () => {
      try {
        if (next) {
          await followArtistAction(artistId, artistSlug);
        } else {
          await unfollowArtistAction(artistId, artistSlug);
        }
        router.refresh();
      } catch {
        setIsFollowing(!next);
      }
    });
  }

  const base =
    "inline-flex items-center px-4 py-2 text-sm transition-colors disabled:opacity-60";
  const variant = isFollowing
    ? "border border-border bg-surface hover:bg-background"
    : "bg-foreground text-background hover:bg-foreground/90";

  return (
    <button
      type="button"
      onClick={handleClick}
      disabled={pending || !isLoaded}
      aria-pressed={isFollowing}
      className={[base, variant, className].filter(Boolean).join(" ")}
    >
      {isFollowing ? "Following" : "Follow"}
    </button>
  );
}

"use client";

import { useState, useTransition } from "react";
import { useRouter } from "next/navigation";
import { useAuth } from "@clerk/nextjs";
import {
  followArtistAction,
  unfollowArtistAction,
} from "@/app/actions/follows";

/**
 * T-052 — follow/unfollow control rendered on `/artists/[slug]`.
 *
 * Signed-out: clicking redirects to sign-in with `redirect_url=` set
 * to the current artist page; the follow itself is *not* queued
 * (anonymous follows land with T-052c).
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
      const redirect = encodeURIComponent(`/artists/${artistSlug}`);
      router.push(`/sign-in?redirect_url=${redirect}`);
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

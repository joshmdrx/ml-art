"use server";

/**
 * T-052 — server actions for the follow graph. Same shape as the
 * collections actions: the client component posts a tiny payload, the
 * server hits the API with the Clerk Bearer token, the relevant routes
 * get revalidated so the badge state stays consistent.
 *
 * Both mutations are idempotent on the API side so the UI doesn't have
 * to guard against double-fires.
 *
 * T-052c adds a no-auth variant: `queueAnonFollowAction` records the
 * intent on the anon_id cookie so the merge-anonymous handler replays
 * it after sign-in.
 */

import { revalidatePath } from "next/cache";
import { followArtist, queueAnonFollow, unfollowArtist } from "@/lib/api";

export async function followArtistAction(
  artistId: string,
  artistSlug: string,
): Promise<void> {
  await followArtist(artistId);
  revalidatePath(`/artists/${artistSlug}`);
  revalidatePath("/studio");
}

export async function unfollowArtistAction(
  artistId: string,
  artistSlug: string,
): Promise<void> {
  await unfollowArtist(artistId);
  revalidatePath(`/artists/${artistSlug}`);
  revalidatePath("/studio");
}

/**
 * T-052c — anon-side capture of a follow intent. Called from the
 * signed-out branch of `<FollowButton>` *before* the redirect to
 * sign-in, so the merge-anonymous handler can replay it once the
 * user comes back. Best-effort: failures are swallowed at the caller
 * (the user can re-click after sign-in).
 *
 * No revalidate — the anon user can't see any UI that reflects this.
 */
export async function queueAnonFollowAction(
  artistId: string,
): Promise<void> {
  await queueAnonFollow(artistId);
}

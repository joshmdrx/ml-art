"use server";

/**
 * T-052 — server actions for the follow graph. Same shape as the
 * collections actions: the client component posts a tiny payload, the
 * server hits the API with the Clerk Bearer token, the relevant routes
 * get revalidated so the badge state stays consistent.
 *
 * Both mutations are idempotent on the API side so the UI doesn't have
 * to guard against double-fires.
 */

import { revalidatePath } from "next/cache";
import { followArtist, unfollowArtist } from "@/lib/api";

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

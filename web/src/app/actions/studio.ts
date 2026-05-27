"use server";

/**
 * Server actions for the studio surface. The PATCH endpoint requires a
 * Clerk Bearer; keeping the call server-side means the token never
 * touches the browser. Same pattern as `actions/collections.ts`.
 */

import { revalidatePath } from "next/cache";
import {
  addStudioArtworkImage,
  createStudioArtwork,
  deleteStudioArtwork,
  getStudioArtwork,
  patchStudioArtwork,
  removeStudioArtworkImage,
  updateStudioSettings as apiUpdateStudioSettings,
  type CreateArtworkBody,
  type PatchArtworkBody,
  type StudioArtist,
  type StudioArtworkDetail,
  type StudioArtworkSummary,
  type StudioImage,
  type StudioSettingsPatch,
} from "@/lib/api";

export async function updateStudioSettings(
  body: StudioSettingsPatch
): Promise<StudioArtist> {
  const updated = await apiUpdateStudioSettings(body);
  // Revalidate paths that surface this artist's profile so the change
  // reflects immediately. Includes the public artist page (which now
  // 404s if status flipped to `paused`).
  revalidatePath("/studio/settings");
  revalidatePath("/studio");
  revalidatePath(`/artists/${updated.slug}`);
  return updated;
}

/** Fetch the studio detail view of an artwork. Wraps the lib/api call
 * so client components can use it (lib/api itself imports server-only
 * Clerk modules and can't be called from client land). */
export async function loadArtworkForEdit(
  id: string
): Promise<StudioArtworkDetail | null> {
  return await getStudioArtwork(id);
}

export async function createArtwork(
  body: CreateArtworkBody
): Promise<StudioArtworkSummary> {
  const created = await createStudioArtwork(body);
  revalidatePath("/studio");
  return created;
}

export async function patchArtwork(
  id: string,
  body: PatchArtworkBody
): Promise<StudioArtworkSummary> {
  const updated = await patchStudioArtwork(id, body);
  // Public artist page also revalidates because a draft → published
  // status flip should reflect immediately.
  revalidatePath("/studio");
  revalidatePath(`/artworks/${id}`);
  return updated;
}

export async function deleteArtwork(id: string): Promise<void> {
  await deleteStudioArtwork(id);
  revalidatePath("/studio");
  revalidatePath(`/artworks/${id}`);
}

export async function addArtworkImage(
  artworkId: string,
  body: { s3_key: string; is_primary?: boolean }
): Promise<StudioImage> {
  const img = await addStudioArtworkImage(artworkId, body);
  revalidatePath("/studio");
  revalidatePath(`/artworks/${artworkId}`);
  return img;
}

export async function removeArtworkImage(
  artworkId: string,
  imageId: string
): Promise<void> {
  await removeStudioArtworkImage(artworkId, imageId);
  revalidatePath("/studio");
  revalidatePath(`/artworks/${artworkId}`);
}

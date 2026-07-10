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
  createStudioLocation,
  deleteStudioArtwork,
  deleteStudioLocation,
  getStripeOnboardingLink,
  getStudioArtwork,
  listStudioLocations,
  markOrderShipped,
  patchStudioArtwork,
  patchStudioLocation,
  removeStudioArtworkImage,
  updateStudioSettings as apiUpdateStudioSettings,
  uploadImageForSearch,
  type CreateArtworkBody,
  type CreateLocationBody,
  type PatchArtworkBody,
  type PatchLocationBody,
  type StudioArtist,
  type StudioArtworkDetail,
  type StudioArtworkSummary,
  type StudioImage,
  type StudioLocation,
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

/**
 * Upload an image file, embed it, and attach it to an artwork.
 *
 * Two-step chain:
 *   1. `POST /v1/uploads/image` — multipart upload → S3/MinIO →
 *      inline Jina embed → `uploads` row + an `s3_key` like
 *      `uploads/<uuid>.<ext>`. (Reuses the visual-search upload
 *      endpoint; same path the camera-icon flow uses.)
 *   2. `POST /v1/studio/artworks/:id/images` with that `s3_key`,
 *      which inserts the `artwork_images` row + re-embeds into
 *      `artwork_embeddings` if it's the primary.
 *
 * The double-embed (once on the upload row, once on the artwork) is
 * wasteful but functionally correct; v1's volume is low enough that
 * we eat it. When the artwork-image surface gets a dedicated
 * "uploads-bucket-aware" path we can drop the duplicate.
 *
 * Rendering: `core::images::url_for_s3_key` now routes any
 * `uploads/`-prefixed key to `UPLOADS_PUBLIC_URL_PREFIX`, so the
 * artwork image displays correctly from the uploads bucket without
 * touching the public artwork rendering path elsewhere.
 */
export async function uploadArtworkImage(
  artworkId: string,
  file: { name: string; type: string; bytes: Uint8Array }
): Promise<StudioImage> {
  const ack = await uploadImageForSearch(file);
  const img = await addStudioArtworkImage(artworkId, { s3_key: ack.s3_key });
  revalidatePath("/studio");
  revalidatePath(`/artworks/${artworkId}`);
  return img;
}

// ─────────────────────────────────────────────────────────────────────────────
// Studio locations (T-038 G3) — server actions for the "Where to see my
// work" section of /studio/settings.
//
// Each mutating action also `revalidatePath`s the public artist page so
// new / edited pins surface there as soon as Mapbox returns a geocode.
// We don't know the artist's slug at the action layer, so we revalidate
// /studio/settings (where the form lives) + a layout-level revalidation
// will pick up the artist page on next request.
// ─────────────────────────────────────────────────────────────────────────────

export async function loadStudioLocations(): Promise<StudioLocation[] | null> {
  return await listStudioLocations();
}

export async function createLocation(
  body: CreateLocationBody
): Promise<StudioLocation> {
  const loc = await createStudioLocation(body);
  revalidatePath("/studio/settings");
  return loc;
}

export async function patchLocation(
  id: string,
  body: PatchLocationBody
): Promise<StudioLocation> {
  const loc = await patchStudioLocation(id, body);
  revalidatePath("/studio/settings");
  return loc;
}

export async function deleteLocation(id: string): Promise<void> {
  await deleteStudioLocation(id);
  revalidatePath("/studio/settings");
}

// ── Marketplace (M-06) ──────────────────────────────────────────────

/** Start (or resume) Stripe Connect onboarding; returns the hosted URL
 * for the client to redirect to. */
export async function startPayoutOnboarding(): Promise<{ url: string }> {
  return getStripeOnboardingLink();
}

/** Mark a paid order shipped. Revalidates the orders surfaces so the
 * status flip shows on navigate-back. */
export async function shipOrder(
  id: string,
  carrier: string,
  trackingNumber: string
): Promise<{ status: string }> {
  const ack = await markOrderShipped(id, carrier, trackingNumber);
  revalidatePath("/studio/orders");
  revalidatePath(`/studio/orders/${id}`);
  return ack;
}

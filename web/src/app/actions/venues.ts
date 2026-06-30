"use server";

/**
 * T-081 — server-action wrappers around the venues + invitation API.
 * Same shape as actions/series.ts: client components call these so the
 * Clerk + cookies server-only modules don't leak into the browser
 * bundle.
 */

import { revalidatePath } from "next/cache";
import {
  createStudioVenue,
  decideVenueRequest as apiDecideVenueRequest,
  deleteStudioVenue,
  getStudioVenue,
  inviteArtworkToVenue,
  listVenueArtworks,
  patchStudioVenue,
  uninviteArtworkFromVenue,
  type CreateVenueBody,
  type PatchVenueBody,
  type Venue,
  type VenueArtworkRow,
} from "@/lib/api";

export async function createVenue(body: CreateVenueBody): Promise<Venue> {
  const v = await createStudioVenue(body);
  revalidatePath("/studio/venues");
  return v;
}

export async function loadVenue(id: string): Promise<Venue | null> {
  return await getStudioVenue(id);
}

export async function updateVenue(
  id: string,
  body: PatchVenueBody,
): Promise<Venue> {
  const v = await patchStudioVenue(id, body);
  revalidatePath("/studio/venues");
  revalidatePath(`/venues/${v.slug}`);
  return v;
}

export async function removeVenue(id: string): Promise<void> {
  await deleteStudioVenue(id);
  revalidatePath("/studio/venues");
}

export async function loadVenueArtworks(
  venueId: string,
): Promise<VenueArtworkRow[]> {
  return await listVenueArtworks(venueId);
}

export async function setVenueArtworks(
  venueId: string,
  artworkIds: string[],
): Promise<void> {
  // No bulk-replace endpoint server-side (intentional — per-row PUT
  // semantics would require more careful conflict handling). Compute
  // the diff client-side via two parallel calls per changed id; the
  // backend's invite is idempotent + the uninvite is forgiving so
  // double-firing is safe.
  const existing = await listVenueArtworks(venueId);
  const existingIds = new Set(existing.map((r) => r.artwork_id));
  const next = new Set(artworkIds);

  const toAdd = artworkIds.filter((id) => !existingIds.has(id));
  const toRemove = [...existingIds].filter((id) => !next.has(id));

  await Promise.all([
    ...toAdd.map((id) => inviteArtworkToVenue(venueId, id)),
    ...toRemove.map((id) => uninviteArtworkFromVenue(venueId, id)),
  ]);
  revalidatePath("/studio/venues");
}

export async function decideVenueRequest(
  venueId: string,
  artworkId: string,
  decision: "accept" | "decline",
): Promise<void> {
  await apiDecideVenueRequest(venueId, artworkId, decision);
  revalidatePath("/studio/venue-requests");
}

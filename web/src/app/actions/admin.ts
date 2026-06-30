"use server";

/**
 * T-083 — server actions for the admin surface (`/admin/*`).
 *
 * Wraps `lib/api`'s admin helpers so client components can call them
 * without pulling Clerk's server-only modules into the browser bundle.
 *
 * Auth gate is the API itself (`AdminUser` extractor returns 403 for
 * non-admins); these actions trust the gate and surface its error
 * message. The web layer's `/admin/*` routes also gate via `getMe()`
 * + `notFound()` so non-admins don't reach the page in the first place.
 */

import { revalidatePath } from "next/cache";
import {
  adminArtistTransition,
  adminImageOverride,
  adminVenueTransition,
  type AdminArtistTransition,
} from "@/lib/api";

export async function transitionArtist(
  id: string,
  action: AdminArtistTransition,
): Promise<{ id: string; status: string }> {
  const result = await adminArtistTransition(id, action);
  // After a transition the admin's queue view + the public artist page
  // both need to reflect the new status.
  revalidatePath("/admin/artists");
  revalidatePath("/admin");
  return result;
}

export async function overrideImageRejection(
  id: string,
): Promise<{ id: string; moderation_status: string }> {
  const result = await adminImageOverride(id);
  revalidatePath("/admin/images");
  revalidatePath("/admin");
  return result;
}

export async function transitionVenue(
  id: string,
  decision: "approve" | "decline",
): Promise<{ id: string; status: string }> {
  const result = await adminVenueTransition(id, decision);
  revalidatePath("/admin/venues");
  revalidatePath("/admin");
  // Public venue surfaces flip visibility on approve.
  revalidatePath("/venues");
  return result;
}

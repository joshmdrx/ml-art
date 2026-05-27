"use server";

/**
 * Server actions for the studio surface. The PATCH endpoint requires a
 * Clerk Bearer; keeping the call server-side means the token never
 * touches the browser. Same pattern as `actions/collections.ts`.
 */

import { revalidatePath } from "next/cache";
import {
  updateStudioSettings as apiUpdateStudioSettings,
  type StudioArtist,
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

"use server";

/**
 * Server actions for the T-058 series surface. Same pattern as
 * `actions/studio.ts` — keeps the Clerk Bearer + cookie threading on
 * the server and lets client components call series mutations without
 * pulling `@clerk/nextjs/server` into the client bundle.
 */

import { revalidatePath } from "next/cache";
import {
  createStudioSeries,
  deleteStudioSeries,
  patchStudioSeries,
  setSeriesArtworks,
  type CreateSeriesBody,
  type PatchSeriesBody,
  type SeriesMembershipAck,
  type StudioSeries,
} from "@/lib/api";

export async function createSeries(
  body: CreateSeriesBody,
): Promise<StudioSeries> {
  const created = await createStudioSeries(body);
  revalidatePath("/studio/series");
  return created;
}

export async function patchSeries(
  id: string,
  body: PatchSeriesBody,
): Promise<StudioSeries> {
  const updated = await patchStudioSeries(id, body);
  revalidatePath("/studio/series");
  revalidatePath(`/artists/${updated.slug}/series/${updated.slug}`);
  return updated;
}

export async function deleteSeries(id: string): Promise<void> {
  await deleteStudioSeries(id);
  revalidatePath("/studio/series");
}

export async function saveSeriesArtworks(
  id: string,
  artwork_ids: string[],
): Promise<SeriesMembershipAck> {
  const ack = await setSeriesArtworks(id, artwork_ids);
  // Membership changes affect the studio list (artwork_count) AND the
  // public series detail page (which artworks render).
  revalidatePath("/studio/series");
  return ack;
}

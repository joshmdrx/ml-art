"use server";

/**
 * Server actions for collection writes. The `<SaveModal>` client component
 * calls these from a form/button — the actual API request happens
 * server-side, which means the Clerk Bearer token never leaves the Next
 * process.
 *
 * Returns plain shapes (not Response) — Next's server-action transport
 * handles serialization. Errors bubble up to the client component, which
 * shows them inline.
 */

import { revalidatePath } from "next/cache";
import {
  addArtworkToCollection,
  createCollection,
  listMyCollections,
  removeArtworkFromCollection,
  type CollectionSummary,
} from "@/lib/api";

export async function fetchMyCollectionsForArtwork(
  artworkId: string
): Promise<{ collections: CollectionSummary[]; saved: Set<string> }> {
  // `artwork_id` opts into per-row membership flags on the API side
  // (`contains_artwork: bool`). Lets the modal render check-state in
  // one round-trip instead of N per-collection lookups.
  const list = await listMyCollections({ artworkId });
  const saved = new Set<string>(
    list.items.filter((c) => c.contains_artwork).map((c) => c.id)
  );
  return { collections: list.items, saved };
}

export async function saveArtworkToCollection(
  collectionId: string,
  artworkId: string
): Promise<void> {
  await addArtworkToCollection(collectionId, artworkId);
  revalidatePath(`/artworks/${artworkId}`);
  revalidatePath(`/collections/${collectionId}`);
  revalidatePath("/collections");
}

export async function unsaveArtworkFromCollection(
  collectionId: string,
  artworkId: string
): Promise<void> {
  await removeArtworkFromCollection(collectionId, artworkId);
  revalidatePath(`/artworks/${artworkId}`);
  revalidatePath(`/collections/${collectionId}`);
  revalidatePath("/collections");
}

export async function createCollectionWithFirstArtwork(
  name: string,
  artworkId: string
): Promise<CollectionSummary> {
  const c = await createCollection({ name });
  await addArtworkToCollection(c.id, artworkId);
  revalidatePath(`/artworks/${artworkId}`);
  revalidatePath("/collections");
  return { ...c, artwork_count: 1 };
}

"use server";

/**
 * T-086 — server actions for the private preview page
 * (`/preview/[token]`). The token in the URL is the credential; claim
 * additionally needs the visitor to be signed in (Clerk Bearer added by
 * `lib/api`).
 */

import { revalidatePath } from "next/cache";
import {
  claimDraftPreview,
  removeDraftPreview,
  type ClaimPreviewResult,
} from "@/lib/api";

export async function claimPreviewAction(
  token: string,
): Promise<ClaimPreviewResult> {
  const result = await claimDraftPreview(token);
  if (result.ok) {
    revalidatePath("/studio", "layout");
  }
  return result;
}

export async function removePreviewAction(token: string): Promise<void> {
  await removeDraftPreview(token);
  revalidatePath(`/preview/${token}`);
}

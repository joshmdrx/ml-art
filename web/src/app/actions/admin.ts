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
  assignDraft,
  createAdminDraft,
  createDraftPreviewLink,
  refundOrder,
  startEditingDraft,
  stopEditingDraft,
  takedownDraft,
  type AdminArtistTransition,
  type AdminDraftItem,
  type AssignDraftResult,
  type CreateDraftBody,
  type DraftPreviewLink,
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

/** M-08 — fire an admin refund; the `charge.refunded` webhook then flips
 * the order to `refunded` + emails the buyer/artist. */
export async function refundOrderAction(
  id: string,
  reason: string,
): Promise<{ refund_id: string; status: string }> {
  const result = await refundOrder(id, reason);
  revalidatePath(`/admin/orders/${id}`);
  revalidatePath("/admin/orders");
  return result;
}

// ── T-086 — consent-first draft profiles ────────────────────────────────

export async function createDraftAction(
  body: CreateDraftBody,
): Promise<AdminDraftItem> {
  const draft = await createAdminDraft(body);
  revalidatePath("/admin/drafts");
  return draft;
}

/** Point the admin's studio at this draft, then the caller navigates to
 * `/studio` to build it with the ordinary studio UI. */
export async function startEditingDraftAction(id: string): Promise<void> {
  await startEditingDraft(id);
  revalidatePath("/admin/drafts");
  revalidatePath("/studio", "layout");
}

export async function stopEditingDraftAction(): Promise<void> {
  await stopEditingDraft();
  revalidatePath("/admin/drafts");
  revalidatePath("/studio", "layout");
}

/** Mint a fresh private link (revokes the previous one). The plaintext
 * token is only available in this response. */
export async function createPreviewLinkAction(
  id: string,
): Promise<DraftPreviewLink> {
  const link = await createDraftPreviewLink(id);
  revalidatePath("/admin/drafts");
  return link;
}

export async function takedownDraftAction(id: string): Promise<void> {
  await takedownDraft(id);
  revalidatePath("/admin/drafts");
  revalidatePath("/studio", "layout");
}

/** Give a draft page to an existing account by email (no link needed). */
export async function assignDraftAction(
  id: string,
  email: string,
): Promise<AssignDraftResult> {
  const result = await assignDraft(id, email);
  if (result.ok) revalidatePath("/admin/drafts");
  return result;
}

"use server";

/**
 * T-068 — server actions for notification preferences.
 *
 * Same shape as other server-action modules: the client component
 * posts a tiny diff, the action calls the Rust API with the Clerk
 * Bearer token, the relevant routes get revalidated so any other
 * surface showing prefs (none today, but reserved for future)
 * picks up the new state.
 */

import { revalidatePath } from "next/cache";
import {
  patchNotificationPreferences,
  type NotificationPreferences,
} from "@/lib/api";

export async function setNotificationPreferences(input: {
  global_enabled?: boolean;
  kinds?: Record<string, boolean>;
}): Promise<NotificationPreferences> {
  const updated = await patchNotificationPreferences(input);
  revalidatePath("/me/settings/notifications");
  return updated;
}

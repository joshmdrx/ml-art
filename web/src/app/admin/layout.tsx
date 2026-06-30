import { notFound } from "next/navigation";
import { auth } from "@clerk/nextjs/server";
import { getMe } from "@/lib/api";

/**
 * T-083 — admin shell. Gate every `/admin/*` route on `users.is_admin`.
 *
 * Two reasons for the layout-level guard:
 *   1. Centralised: one place to maintain the gate; child pages don't
 *      each repeat the check.
 *   2. Defensive: even if a child page forgets to call `getMe()`, the
 *      layout has already 404'd for non-admins.
 *
 * Non-admins get `notFound()` (not a "you don't have access" page) so
 * the admin surface stays invisible. The API still returns 403 if a
 * non-admin reaches `/v1/admin/*` directly; the layout's 404 is purely
 * about hiding the existence of the surface from a casual visitor.
 */
export default async function AdminLayout({
  children,
}: {
  children: React.ReactNode;
}) {
  const { userId } = await auth();
  if (!userId) notFound();
  const me = await getMe().catch(() => null);
  if (!me?.is_admin) notFound();
  return children;
}

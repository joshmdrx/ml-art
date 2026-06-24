import Link from "next/link";
import { auth } from "@clerk/nextjs/server";
import { getStudioMe } from "@/lib/api";
import { reportError } from "@/lib/reportError";
import { UnreadBadge } from "@/components/ui/UnreadBadge";

/**
 * TopNav "Studio" link, optionally badged with the unread-inquiry
 * count (T-074).
 *
 * Async server component. Server-renders on every page navigation,
 * which is also when the count refreshes — no polling, no
 * WebSockets, no client-side state. If an inquiry arrives while the
 * artist is sitting on a single page, the badge updates on their
 * next nav.
 *
 *   - Signed-out → renders null (the parent <Show when="signed-in">
 *     already gates this, but we double-check defensively so a brief
 *     SSR-vs-CSR auth-state mismatch never leaks an extra `/v1/studio/me`
 *     call for anonymous users).
 *   - Signed-in non-artist → renders the link with no badge. /v1/studio/me
 *     returns null in `lib/api.ts` for the 404 case.
 *   - Signed-in artist → renders the link, plus an `<UnreadBadge>` when
 *     `unread_inquiry_count > 0`. Capped at "9+" in the badge component
 *     so the visible width is bounded.
 *
 * The fetch is wrapped in try/catch → reportError → graceful no-badge
 * fallback. The badge is non-critical UI; a `/v1/studio/me` blip should
 * NOT break the nav.
 */
export async function StudioNavLink() {
  const { userId } = await auth();
  if (!userId) return null;

  const me = await getStudioMe().catch((e) => {
    reportError(e, { surface: "top-nav", call: "studio-me" });
    return null;
  });

  // Non-artist signed-in users (collectors who haven't onboarded as
  // an artist) get the link but no count — the link itself leads to
  // /studio which redirects them through onboarding.
  const count = me?.unread_inquiry_count ?? 0;

  return (
    <Link
      href="/studio"
      className="inline-flex items-center text-muted hover:text-foreground"
    >
      Studio
      <UnreadBadge
        count={count}
        label={
          count === 1
            ? "1 unread inquiry"
            : `${count} unread inquiries`
        }
      />
    </Link>
  );
}

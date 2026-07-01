import { TopNav } from "@/components/TopNav";
import { StudioSidebar } from "@/components/StudioSidebar";
import { getStudioMe } from "@/lib/api";
import { reportError } from "@/lib/reportError";

/**
 * `/studio/*` layout — sidebar-flavoured shell that wraps every studio
 * subpage. Fetches `/v1/studio/me` once here so the sidebar can badge
 * the Inquiries item with the caller's unread count.
 *
 * Non-artist users (bob-style — signed in but no artist row) get
 * `null` from `getStudioMe()`; the individual subpages catch this and
 * redirect to `/onboarding`. Layout renders the sidebar shell either
 * way — the redirects happen before children render.
 */
export default async function StudioLayout({
  children,
}: {
  children: React.ReactNode;
}) {
  const me = await getStudioMe().catch((e) => {
    reportError(e, { surface: "studio-layout", call: "me" });
    return null;
  });
  const unread = me?.unread_inquiry_count ?? 0;
  // Non-artist users (signed-in but no artist row) get null → the
  // sidebar hides the "View public page" affordance since there's
  // no page to view yet.
  const publicSlug = me?.slug ?? null;

  return (
    <>
      <TopNav />
      <div className="flex-1 flex flex-col lg:flex-row w-full mx-auto max-w-screen-2xl">
        <StudioSidebar unreadInquiries={unread} publicSlug={publicSlug} />
        <main className="flex-1 min-w-0 px-6 py-8 lg:py-10">{children}</main>
      </div>
    </>
  );
}

"use client";

/**
 * Studio sidebar navigation.
 *
 * Responsive shape:
 *   - Desktop (`lg:`+): fixed left column, ~14rem wide, sticky within
 *     the studio layout.
 *   - Mobile / tablet: horizontal tab strip above the content. Same
 *     links, same active-state styling — just laid out horizontally
 *     to conserve vertical real estate.
 *
 * Active state uses `usePathname()` so navigation between studio
 * subpages highlights the right item without a hard nav. Exact match
 * for `/studio` (portfolio); prefix match for the others (a nested
 * page like `/studio/series?id=…` still highlights Series).
 */

import Link from "next/link";
import { usePathname } from "next/navigation";
import { clsx } from "clsx";
import { UnreadBadge } from "@/components/ui/UnreadBadge";

type Item = {
  href: string;
  label: string;
  /** When true, the active check requires an exact match. Portfolio
   * lives at `/studio` — a prefix match would also flag it as active
   * on every sub-page. */
  exact?: boolean;
  /** Optional unread count for the badge. */
  count?: number;
};

export function StudioSidebar({
  unreadInquiries,
  publicSlug,
}: {
  unreadInquiries: number;
  /** Artist's public slug, e.g. `alice-test`. Null for signed-in
   * users who don't yet have an artist row (bob-style) — the "View
   * public page" affordance is hidden in that case. */
  publicSlug: string | null;
}) {
  const pathname = usePathname();
  const items: Item[] = [
    { href: "/studio", label: "Portfolio", exact: true },
    { href: "/studio/series", label: "Series" },
    { href: "/studio/inquiries", label: "Inquiries", count: unreadInquiries },
    { href: "/studio/orders", label: "Orders" },
    { href: "/studio/settings", label: "Settings" },
  ];

  function isActive(item: Item): boolean {
    if (item.exact) return pathname === item.href;
    return pathname === item.href || pathname.startsWith(`${item.href}/`);
  }

  return (
    <nav
      aria-label="Studio navigation"
      className="lg:w-56 lg:shrink-0 lg:border-r lg:border-border lg:min-h-[calc(100vh-4rem)] lg:sticky lg:top-16 lg:self-start"
    >
      {/* Mobile: horizontal strip. Desktop: vertical list. */}
      <ul className="flex lg:flex-col gap-1 overflow-x-auto lg:overflow-visible lg:py-6 px-6 lg:px-4 py-3 border-b lg:border-b-0 border-border">
        {items.map((item) => {
          const active = isActive(item);
          return (
            <li key={item.href} className="shrink-0">
              <Link
                href={item.href}
                aria-current={active ? "page" : undefined}
                className={clsx(
                  "flex items-center justify-between gap-2 px-3 py-2 text-sm transition-colors whitespace-nowrap",
                  active
                    ? "bg-foreground text-background"
                    : "text-muted hover:text-foreground hover:bg-background",
                )}
              >
                <span>{item.label}</span>
                {typeof item.count === "number" && item.count > 0 && (
                  <UnreadBadge
                    count={item.count}
                    label={`${item.count} unread inquiries`}
                  />
                )}
              </Link>
            </li>
          );
        })}
      </ul>

      {/* Utility links — desktop only; mobile users have the top-nav
          for site navigation. "View public page" only renders when the
          caller has an artist row (signed-in-but-not-onboarded users
          have nowhere to go yet). */}
      <div className="hidden lg:flex lg:flex-col px-4 py-6 mt-auto gap-2">
        {publicSlug && (
          <Link
            href={`/artists/${encodeURIComponent(publicSlug)}`}
            target="_blank"
            rel="noopener noreferrer"
            className="text-xs text-muted hover:text-foreground"
          >
            View public page ↗
          </Link>
        )}
        <Link
          href="/"
          className="text-xs text-muted hover:text-foreground"
        >
          ← Back to site
        </Link>
      </div>
    </nav>
  );
}

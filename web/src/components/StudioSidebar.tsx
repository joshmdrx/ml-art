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
}: {
  unreadInquiries: number;
}) {
  const pathname = usePathname();
  const items: Item[] = [
    { href: "/studio", label: "Portfolio", exact: true },
    { href: "/studio/series", label: "Series" },
    { href: "/studio/inquiries", label: "Inquiries", count: unreadInquiries },
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

      {/* Exit link — desktop only; mobile users have the top-nav
          Wander logo to get back to the public site. */}
      <div className="hidden lg:block px-4 py-6 mt-auto">
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

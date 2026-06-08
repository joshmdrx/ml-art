/**
 * Pure formatters reused across server + client components.
 *
 * Lives separately from `lib/api.ts` so client components (e.g.
 * `ArtworkCard`, which is now in the client graph via
 * `SearchSidePanel`) can call them without pulling `lib/api.ts`'s
 * Clerk-server dynamic import into the client bundle. The Clerk
 * module is `server-only` and Next.js correctly refuses to bundle
 * it into a Client Component — splitting the formatters out is the
 * fix.
 */

import type { Dimensions } from "@/lib/api";

export function formatDimensions(d: Dimensions | null): string | null {
  if (!d || (d.height == null && d.width == null)) return null;
  const unit = d.unit ?? "cm";
  const parts = [d.height, d.width, d.depth]
    .filter((n): n is number => typeof n === "number")
    .map((n) => `${n}`);
  if (parts.length === 0) return null;
  return `${parts.join(" × ")} ${unit}`;
}

export function formatPrice(
  cents: number | null,
  currency: string
): string | null {
  if (cents === null) return null;
  const major = cents / 100;
  try {
    return new Intl.NumberFormat("en-US", {
      style: "currency",
      currency,
      maximumFractionDigits: 0,
    }).format(major);
  } catch {
    return `${major.toFixed(0)} ${currency}`;
  }
}

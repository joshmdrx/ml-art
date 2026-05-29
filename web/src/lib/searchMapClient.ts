/**
 * Browser-only fetch wrapper for `/v1/search/map` (T-038 G5).
 *
 * Lives in its own module so the SearchMap client component can call it
 * without dragging in `lib/api.ts`'s `apiFetch` — which dynamically
 * imports `@clerk/nextjs/server`. The Bearer header isn't needed for
 * this endpoint (it's a public read), so the simpler `window.fetch`
 * path is correct and avoids the server-only Clerk module showing up in
 * the client bundle. See `decisions.md` 2026-05-27 — client/server
 * import boundaries.
 */

import type { MapPin, MapSearchParams } from "@/lib/api";

const API_BASE_URL =
  process.env.NEXT_PUBLIC_API_BASE_URL ?? "http://localhost:9100";

export async function searchMapClient(
  params: MapSearchParams
): Promise<MapPin[]> {
  const usp = new URLSearchParams();
  for (const [k, v] of Object.entries(params)) {
    if (typeof v === "string" && v.length > 0) usp.set(k, v);
  }
  const qs = usp.toString();
  const res = await fetch(
    `${API_BASE_URL}/v1/search/map${qs ? `?${qs}` : ""}`,
    {
      // Cache disabled — pan/zoom triggers a fresh fetch every move.
      // Browsers will respect HTTP caching headers if the API ever
      // sends them; we don't try to second-guess here.
      cache: "no-store",
    }
  );
  if (!res.ok) {
    const text = await res.text().catch(() => "");
    throw new Error(`search/map ${res.status}: ${text || res.statusText}`);
  }
  return (await res.json()) as MapPin[];
}

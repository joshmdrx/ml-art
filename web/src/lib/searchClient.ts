/**
 * Browser-only fetch wrapper for `/v1/search` (T-037 cursor pagination).
 *
 * Mirrors `searchMapClient.ts` rather than reusing `lib/api.ts`'s
 * `searchArtworks` — that one goes through `apiFetch`, which
 * dynamically imports `@clerk/nextjs/server` to attach the Bearer
 * token. `/v1/search` is a public read; client-side pagination
 * doesn't need auth and shouldn't drag a server-only module into
 * the browser bundle. See decisions.md 2026-05-27.
 */

import type {
  ArtworkSummary,
  Paginated,
  SearchParams,
} from "@/lib/api";

const API_BASE_URL =
  process.env.NEXT_PUBLIC_API_BASE_URL ?? "http://localhost:9100";

export async function searchClient(
  params: SearchParams,
): Promise<Paginated<ArtworkSummary>> {
  const usp = new URLSearchParams();
  for (const [k, v] of Object.entries(params)) {
    if (v === undefined || v === null) continue;
    if (typeof v === "string" && v.length === 0) continue;
    usp.set(k, String(v));
  }
  const qs = usp.toString();
  const res = await fetch(
    `${API_BASE_URL}/v1/search${qs ? `?${qs}` : ""}`,
    { cache: "no-store" },
  );
  if (!res.ok) {
    const text = await res.text().catch(() => "");
    throw new Error(`search ${res.status}: ${text || res.statusText}`);
  }
  return (await res.json()) as Paginated<ArtworkSummary>;
}

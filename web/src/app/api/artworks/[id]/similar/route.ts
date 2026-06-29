/**
 * `GET /api/artworks/:id/similar` — T-063 bridge route.
 *
 * Thin GET proxy to `api.wander.gallery/v1/artworks/:id/similar`.
 * The api endpoint is public (no auth), but we still route through
 * the bridge so the browser never reaches across origins — keeps the
 * fetch same-origin and avoids CORS headaches if we ever lock down
 * the api Function URL (T-064).
 *
 * Called by `<ArtworkCard>`'s hover-flyout on the grid surfaces.
 */

import { NextResponse } from "next/server";
import { getSimilarArtworks } from "@/lib/api";
import { reportError } from "@/lib/reportError";

const MAX_LIMIT = 8;

export async function GET(
  req: Request,
  { params }: { params: Promise<{ id: string }> },
) {
  const { id } = await params;
  const url = new URL(req.url);
  const limit = Math.min(
    MAX_LIMIT,
    Math.max(1, parseInt(url.searchParams.get("limit") ?? "4", 10) || 4),
  );

  try {
    const result = await getSimilarArtworks(id, { limit });
    return NextResponse.json(result);
  } catch (e) {
    reportError(e, { surface: "similar-bridge", artwork_id: id });
    return NextResponse.json(
      { items: [], next_cursor: null },
      { status: 502 },
    );
  }
}

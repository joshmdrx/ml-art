/**
 * `POST /api/events` — T-050.3 client-events bridge.
 *
 * Browser → Next.js → api.wander.gallery. Same-origin proxy so the
 * browser's `anon_id` cookie reaches the api side (a direct cross-
 * origin fetch from the browser to api.wander.gallery doesn't carry
 * wander.gallery cookies). `lib/api::apiFetch` handles the cookie +
 * Clerk-token forwarding.
 *
 * Trust model lives on the api side (`api-search::events::ingest`):
 *   - allowlist of event names (server-only events 400)
 *   - max-batch cap (50 events)
 *   - server derives identity from the cookie/JWT, never the body
 *
 * We forward the body verbatim. Bad shapes → the api returns 400/422
 * and we mirror that status back to the client.
 */

import { NextResponse } from "next/server";

import { postEvents } from "@/lib/api";
import { reportError } from "@/lib/reportError";

export async function POST(req: Request) {
  let body: unknown;
  try {
    body = await req.json();
  } catch {
    return NextResponse.json({ error: "invalid_json" }, { status: 400 });
  }

  try {
    const res = await postEvents(body);
    // 2xx — propagate. We don't return a body; the client doesn't
    // need one and saving bytes matters at events scale.
    return new NextResponse(null, { status: res.status });
  } catch (e) {
    reportError(e, { surface: "events-bridge" });
    return NextResponse.json({ error: "events_bridge_failed" }, { status: 502 });
  }
}

/**
 * `POST /api/me/merge-anonymous` — T-033 bridge.
 *
 * Client-side fire-and-forget endpoint. The signed-in user's browser
 * POSTs here once per session (gated client-side by sessionStorage).
 * This handler then makes the server→API call carrying the Clerk
 * bearer + anon-id cookie automatically via `apiFetch`.
 *
 * Why a route handler instead of a server action: server actions are
 * tied to React form state, and we want a tiny no-UI bridge that can
 * be fired imperatively from a small client component on mount.
 */
import { NextResponse } from "next/server";
import { auth } from "@clerk/nextjs/server";
import { mergeAnonymous } from "@/lib/api";
import { reportError } from "@/lib/reportError";

export async function POST() {
  // Drop unauthenticated calls fast — the underlying API would 401 too,
  // but bouncing here keeps the API logs tidy.
  const { userId } = await auth();
  if (!userId) {
    return NextResponse.json({ error: "unauthenticated" }, { status: 401 });
  }

  try {
    const result = await mergeAnonymous();
    return NextResponse.json(result);
  } catch (e) {
    reportError(e, { surface: "merge-anonymous-bridge" });
    // Surface a 502 rather than letting it 500 — failure here is
    // recoverable (the client just retries next session) and we don't
    // want it to look like a generic server bug.
    return NextResponse.json({ error: "merge_failed" }, { status: 502 });
  }
}

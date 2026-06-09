/**
 * `POST /api/studio/inquiries/read` — T-011 Phase 4b bridge.
 *
 * Fired on inbox view from the client component below the page —
 * marks the visible unread inquiries as read. Best-effort: the
 * underlying API is idempotent (re-marking is a no-op) and a
 * dropped request just leaves the rows unread until next view.
 */
import { NextResponse } from "next/server";
import { auth } from "@clerk/nextjs/server";

import { markStudioInquiriesRead } from "@/lib/api";
import { reportError } from "@/lib/reportError";

export async function POST(req: Request) {
  const { userId } = await auth();
  if (!userId) {
    return NextResponse.json({ error: "unauthenticated" }, { status: 401 });
  }

  let body: { ids?: unknown };
  try {
    body = (await req.json()) as { ids?: unknown };
  } catch {
    return NextResponse.json({ error: "invalid_json" }, { status: 400 });
  }

  // Coerce to a string[] defensively — if the client posts garbage
  // we drop bad entries rather than 400ing on the whole batch.
  const ids = Array.isArray(body.ids)
    ? body.ids.filter((v): v is string => typeof v === "string")
    : [];
  if (ids.length === 0) {
    return NextResponse.json({ updated: 0 });
  }

  try {
    const result = await markStudioInquiriesRead(ids);
    return NextResponse.json(result);
  } catch (e) {
    reportError(e, {
      surface: "studio-inquiries-mark-read-bridge",
      count: ids.length,
    });
    return NextResponse.json({ error: "mark_read_failed" }, { status: 502 });
  }
}

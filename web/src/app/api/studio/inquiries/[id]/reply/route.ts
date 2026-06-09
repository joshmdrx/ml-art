/**
 * `POST /api/studio/inquiries/:id/reply` — T-011 Phase 4b bridge.
 *
 * Browser-fired reply submission. Same shape as the
 * merge-anonymous bridge (route handler → `apiFetch` → API): the
 * client component can use plain `fetch` without dragging the
 * server-only Clerk module into the bundle.
 */
import { NextResponse } from "next/server";
import { auth } from "@clerk/nextjs/server";

import { postStudioInquiryReply } from "@/lib/api";
import { reportError } from "@/lib/reportError";

export async function POST(
  req: Request,
  ctx: { params: Promise<{ id: string }> },
) {
  const { userId } = await auth();
  if (!userId) {
    return NextResponse.json({ error: "unauthenticated" }, { status: 401 });
  }

  const { id } = await ctx.params;

  let body: { message?: unknown };
  try {
    body = (await req.json()) as { message?: unknown };
  } catch {
    return NextResponse.json({ error: "invalid_json" }, { status: 400 });
  }
  const message = typeof body.message === "string" ? body.message : "";
  if (!message.trim()) {
    return NextResponse.json({ error: "message_required" }, { status: 400 });
  }

  try {
    const reply = await postStudioInquiryReply(id, message);
    return NextResponse.json(reply);
  } catch (e) {
    // Match the API's status code where we can read it from the
    // error message; otherwise 502 (recoverable upstream failure).
    const msg = e instanceof Error ? e.message : String(e);
    if (msg.includes(" 404:")) {
      return NextResponse.json({ error: "not_found" }, { status: 404 });
    }
    if (msg.includes(" 400:")) {
      return NextResponse.json({ error: "bad_request" }, { status: 400 });
    }
    reportError(e, {
      surface: "studio-inquiry-reply-bridge",
      inquiry_id: id,
    });
    return NextResponse.json({ error: "reply_failed" }, { status: 502 });
  }
}

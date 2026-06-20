/**
 * T-068 — public one-click unsubscribe.
 *
 * Two callers expected:
 *
 *   - GET: a user clicked the footer link in an email. We POST the token
 *     to the API, then redirect to a confirmation page (also under /u/)
 *     so the user sees what happened. (HTTP semantics frown on GET with
 *     side effects, but every email-unsubscribe link in the world is
 *     a GET, so users expect it.)
 *
 *   - POST: an email client (Gmail / Outlook) honouring RFC 8058
 *     `List-Unsubscribe-Post: List-Unsubscribe=One-Click`. They POST
 *     here with a small body; we just need a 2xx back.
 *
 * Both call the same API endpoint and end up in the same place
 * (preference flipped off). Only the response shape differs.
 */

import { NextResponse } from "next/server";
import { unsubscribeWithToken } from "@/lib/api";

interface Params {
  token: string;
}

export async function POST(
  _req: Request,
  context: { params: Promise<Params> },
) {
  const { token } = await context.params;
  try {
    await unsubscribeWithToken(token);
    return new NextResponse(null, { status: 204 });
  } catch {
    // Mail clients don't render error bodies; just return 400 so they
    // know the action didn't take. We deliberately don't leak the
    // reason (expired vs malformed vs unknown-kind) — the user got
    // here from email, they can't act on detail.
    return new NextResponse(null, { status: 400 });
  }
}

export async function GET(
  _req: Request,
  context: { params: Promise<Params> },
) {
  const { token } = await context.params;
  // Forward the user to a confirmation page that does the actual
  // unsubscribe on render. The page is a server component that calls
  // the same API endpoint and renders friendly copy. We can't redirect
  // to it cleanly with the token in the URL (we are the token URL),
  // so we delegate via a query param.
  const url = new URL("/u/confirm", _req.url);
  url.searchParams.set("token", token);
  return NextResponse.redirect(url, 302);
}

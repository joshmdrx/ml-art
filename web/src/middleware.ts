/**
 * Edge middleware — runs on every request before route handlers / server
 * components. Two concerns, composed in one Clerk wrapper:
 *
 *   1. Clerk session setup (so `<SignedIn>` / `<SignedOut>` and `auth()`
 *      work in server components)
 *   2. Anonymous-id signed cookie for behavior tracking and rate limiting
 *      (see `decisions.md` 2026-05-26 — "anonymous identity: cookie at Next,
 *      header to API")
 */

import { clerkMiddleware } from "@clerk/nextjs/server";
import { NextResponse } from "next/server";
import {
  ANON_COOKIE_MAX_AGE_SECONDS,
  ANON_COOKIE_NAME,
  generateUuidV7,
  signAnonId,
  verifyAnonId,
} from "@/lib/anonId";

export default clerkMiddleware(async (_auth, req) => {
  const res = NextResponse.next();

  const existing = req.cookies.get(ANON_COOKIE_NAME)?.value;
  const valid = existing ? await verifyAnonId(existing) : null;

  if (!valid) {
    // No cookie, or tampered/malformed — issue a fresh one.
    const uuid = generateUuidV7();
    const cookieValue = await signAnonId(uuid);
    res.cookies.set({
      name: ANON_COOKIE_NAME,
      value: cookieValue,
      httpOnly: true,
      sameSite: "lax",
      path: "/",
      maxAge: ANON_COOKIE_MAX_AGE_SECONDS,
    });
  }

  return res;
});

export const config = {
  // Match everything except Next internals and obvious static assets.
  matcher: [
    "/((?!_next|[^?]*\\.(?:html?|css|js(?!on)|jpe?g|webp|png|gif|svg|ttf|woff2?|ico|csv|docx?|xlsx?|zip|webmanifest)).*)",
    "/__clerk/(.*)",
    "/(api|trpc)(.*)",
  ],
};

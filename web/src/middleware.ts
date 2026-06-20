/**
 * Edge middleware — runs on every request before route handlers / server
 * components. Two concerns, composed in one Clerk wrapper:
 *
 *   1. Clerk session setup (so `<SignedIn>` / `<SignedOut>` and `auth()`
 *      work in server components).
 *   2. Anonymous-id signed cookie for behaviour tracking and rate limiting
 *      (see `decisions.md` 2026-05-26 — "anonymous identity: cookie at
 *      Next, header to API").
 *
 * Note on direct API Gateway URL hits
 * -----------------------------------
 * The Lambda's view of `Host` is pinned to `wander.gallery` by API
 * Gateway parameter mapping (see `infra/modules/web/main.tf`), so
 * direct hits to the API Gateway invoke URL serve the canonical
 * content correctly. We deliberately do NOT redirect such hits here —
 * we tried, and API Gateway's response handling rewrote our absolute
 * `Location: https://wander.gallery/…` header back to a relative path,
 * which the browser then resolved against the API Gateway URL it was
 * actually on, producing an infinite redirect loop. The proper fix is
 * to restrict the API Gateway invoke URL to CloudFront-only access
 * via a shared-secret header — tracked as a follow-up.
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

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
 * Plus one outer concern: 308-redirecting direct hits on the API
 * Gateway invoke URL back to `wander.gallery`. The Lambda's view of
 * `Host` is pinned to `wander.gallery` by API Gateway parameter mapping
 * (see `infra/modules/web/main.tf`), so we can't detect "direct hit"
 * from the request host any more — `X-Amz-Cf-Id` (set only by
 * CloudFront when it proxies) is the reliable marker.
 */

import { clerkMiddleware } from "@clerk/nextjs/server";
import {
  NextResponse,
  type NextFetchEvent,
  type NextRequest,
} from "next/server";
import {
  ANON_COOKIE_MAX_AGE_SECONDS,
  ANON_COOKIE_NAME,
  generateUuidV7,
  signAnonId,
  verifyAnonId,
} from "@/lib/anonId";

const CANONICAL_HOST = "wander.gallery";

const inner = clerkMiddleware(async (_auth, req) => {
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

export default async function middleware(
  req: NextRequest,
  event: NextFetchEvent,
) {
  // Direct hit on the API Gateway invoke URL — stale bookmark,
  // search-engine indexed URL, autocomplete entry. 308 → wander.gallery
  // so the address bar heals and we don't get duplicate-content SEO.
  // Guarded on `AWS_LAMBDA_FUNCTION_NAME` so local dev (where
  // X-Amz-Cf-Id is also absent) doesn't get redirected.
  if (
    process.env.AWS_LAMBDA_FUNCTION_NAME &&
    !req.headers.has("x-amz-cf-id")
  ) {
    const reqUrl = new URL(req.url);
    const canonical = new URL(
      reqUrl.pathname + reqUrl.search,
      `https://${CANONICAL_HOST}`,
    );
    return NextResponse.redirect(canonical, 308);
  }

  return inner(req, event);
}

export const config = {
  // Match everything except Next internals and obvious static assets.
  matcher: [
    "/((?!_next|[^?]*\\.(?:html?|css|js(?!on)|jpe?g|webp|png|gif|svg|ttf|woff2?|ico|csv|docx?|xlsx?|zip|webmanifest)).*)",
    "/__clerk/(.*)",
    "/(api|trpc)(.*)",
  ],
};

/**
 * T-065 — Sentry init for the server + edge runtimes.
 *
 * Next 15+ picks this file up automatically via the
 * `instrumentation.ts` convention. We don't gate on
 * `process.env.SENTRY_DSN` here — `Sentry.init({ dsn: undefined })` is
 * itself a no-op, which matches the "graceful degradation when a
 * secret is missing" pattern the Rust side uses.
 *
 * Only the server + edge runtimes hit this file — the browser is
 * initialised separately in `instrumentation-client.ts`.
 */

import * as Sentry from "@sentry/nextjs";

export async function register() {
  Sentry.init({
    dsn: process.env.SENTRY_DSN,
    environment: process.env.NODE_ENV,
    // Traces: capture ~5% pre-launch so the free tier doesn't fill
    // up on background traffic. Bump when we have real users + a
    // reason to trace performance.
    tracesSampleRate: 0.05,
    // No source maps yet — see the T-065 follow-up. Errors surface
    // with unminified names for server-side code (Next SSR is bundled
    // but not name-minified in production).
    sendDefaultPii: false,
  });
}

// App-router only — no need for the `onRequestError` hook that
// pages-router projects use. Errors in Server Components + Route
// Handlers are caught by Sentry's Next.js integration automatically.
export const onRequestError = Sentry.captureRequestError;

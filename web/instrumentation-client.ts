/**
 * T-065 — Sentry init for the browser runtime.
 *
 * Next 15+ auto-loads this before the app renders. DSN is a *public*
 * value (that's the whole point — the browser has to know where to
 * send events) so we ship it via `NEXT_PUBLIC_SENTRY_DSN` baked in
 * at build time.
 *
 * Not gated on presence — `dsn: undefined` is a Sentry no-op.
 */

import * as Sentry from "@sentry/nextjs";

Sentry.init({
  dsn: process.env.NEXT_PUBLIC_SENTRY_DSN,
  environment: process.env.NODE_ENV,
  // Pre-launch sampling: capture every error but only 5% of
  // performance traces. Adjust when we have real users.
  tracesSampleRate: 0.05,
  // Replay is off — high value later but adds ~50KB to the browser
  // bundle and is overkill pre-launch.
  replaysSessionSampleRate: 0,
  replaysOnErrorSampleRate: 0,
  sendDefaultPii: false,
});

// Router transitions — Sentry needs the hook wired up manually since
// app-router doesn't fire the pages-router events it used to listen
// to. This gives us "Which route was the user on when the error
// fired?" attribution.
export const onRouterTransitionStart = Sentry.captureRouterTransitionStart;

import type { NextConfig } from "next";
import { withSentryConfig } from "@sentry/nextjs";

const nextConfig: NextConfig = {
  // Standalone build output — required by OpenNext. Previously
  // OpenNext set NEXT_PRIVATE_STANDALONE=true internally when it ran
  // `next build`. Now that `deploy-web.sh` runs `pnpm build` itself
  // (to interpose the T-065 instrumentation-copy workaround before
  // OpenNext's copyTracedFiles), we need to declare standalone here.
  output: "standalone",
  // OpenNext + Next.js tree-shaking strips @swc/helpers' cjs/* files
  // even though Next's compiled output requires them at runtime.
  // Force-include the whole package in the output trace so OpenNext's
  // bundler picks it up. Without this, the deployed Lambda 500s with
  //   `Cannot find module '/var/task/node_modules/@swc/helpers/cjs/_interop_require_default.cjs'`.
  // See: https://github.com/opennextjs/opennextjs-aws/issues — recurring
  // pnpm-tree-shaking interaction.
  // Force-include Next's runtime deps that the bundler otherwise
  // tree-shakes away under pnpm. Symptoms surface as
  //   `Cannot find module '@swc/helpers/cjs/_interop_require_default.cjs'`
  //   `Cannot find module '@next/env'`
  // Including the *whole packages* (not `next/dist/**/*` — that's
  // ~150MB) is the minimal durable fix.
  outputFileTracingIncludes: {
    "**/*": [
      "./node_modules/@swc/helpers/**/*",
      "./node_modules/@next/env/**/*",
    ],
  },
};

// T-065 — Sentry wrapping. The historical block was @sentry/nextjs's
// build-time injection of a `pages/_error` stub, which OpenNext's
// `copyTracedFiles` couldn't reconcile with our app-router-only
// project. Sentry 10.x's app-router path skips that injection when
// there's no `pages/` dir in the project (we have none — see
// `web/src/` layout).
export default withSentryConfig(nextConfig, {
  // Silences the Sentry build plugin banner unless we're debugging.
  silent: !process.env.CI && !process.env.SENTRY_DEBUG,
  // Source-map upload requires `SENTRY_AUTH_TOKEN`. Not wired yet —
  // stack traces will show minified names until we add the token to
  // SSM + the deploy script. Errors still land in Sentry either way.
  sourcemaps: {
    disable: true,
  },
});

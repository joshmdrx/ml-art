import type { NextConfig } from "next";

// Sentry on the web tier is intentionally deferred. @sentry/nextjs 10.x
// injects a page-router `_error` stub during its post-build pass which
// OpenNext 4.0's `copyTracedFiles` can't reconcile with our app-router
// project ("This error should only happen for static 404 and 500 page
// from page router"). The Rust API + jobs Lambdas already report into
// Sentry via the `sentry` crate; web errors still surface as 5xx in
// the CloudWatch + CloudFront alarms. Revisit once OpenNext picks up
// app-router-only Sentry support.

const nextConfig: NextConfig = {
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

export default nextConfig;

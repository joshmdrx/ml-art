import type { NextConfig } from "next";

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

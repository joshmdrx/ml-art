/**
 * OpenNext config. Defaults are right for our shape:
 *   - one server lambda handles SSR + RSC + route handlers
 *   - no ISR cache yet (v1 traffic is too low for ISR to matter)
 *   - no image-optimization function (using CloudFront + S3 directly
 *     via images.wander.gallery; <Image> will rewrite srcs there)
 *
 * Things to revisit:
 *   - ISR: enable `incrementalCache: 's3-lite'` once we have traffic
 *     that justifies cache lookups (~10 req/s sustained).
 *   - Image optimization function: only needed if we start using
 *     Next's <Image> on URLs OpenNext can't pre-rewrite.
 *   - Edge runtime for select routes: middleware already runs at
 *     CloudFront-edge via OpenNext's default; route-level edge
 *     conversion is per-route opt-in.
 */
import type { OpenNextConfig } from "@opennextjs/aws/types/open-next";

// T-065 — buildCommand override. Next 16 doesn't copy
// `instrumentation.js` into the standalone output (only its .nft.json
// trace), which makes OpenNext's `copyTracedFiles` throw. Workaround:
// let `deploy-web.sh` run `pnpm build` itself, copy the missing file
// into the standalone dir, then invoke OpenNext with `buildCommand`
// pointed at a no-op so OpenNext doesn't clobber the fix by
// rebuilding. The no-op reads `.next/standalone/.next/server/…`
// as-is.
const config: OpenNextConfig = {
  buildCommand: "echo 'skipping open-next internal build (pre-built by deploy-web.sh)'",
  default: {},
};

export default config;

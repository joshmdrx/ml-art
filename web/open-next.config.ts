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

// Empty `default` block = stock OpenNext defaults, which already
// skip the warmer lambda. Add overrides here when we need ISR,
// edge runtime, or image-optimization function.
const config: OpenNextConfig = {
  default: {},
};

export default config;

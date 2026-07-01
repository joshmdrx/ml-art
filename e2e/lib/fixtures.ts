import { request as playwrightRequest, expect } from "@playwright/test";

/**
 * Client for the test-fixture insert endpoints
 * (`api/crates/api-search/src/testfixtures.rs`).
 *
 * The endpoints are direct DB inserts guarded by
 * `WANDER_TEST_FIXTURES_ENABLED` on the API side; when the env var
 * isn't set the routes never register, so a helper call in that
 * environment 404s hard. That's intentional — we'd rather fail
 * loudly at test time than silently render an unseeded fixture.
 *
 * API URL resolves from `WANDER_TEST_FIXTURES_API_URL` (CI can
 * override) → falls back to `http://localhost:9100` (dev default).
 * The web layer's `NEXT_PUBLIC_API_BASE_URL` is the same URL in
 * both envs — CI's e2e workflow sets it to `http://localhost:9100`
 * as well — so keeping this helper's default in sync is trivial.
 */

const API_URL =
  process.env.WANDER_TEST_FIXTURES_API_URL ??
  process.env.E2E_API_URL ??
  "http://localhost:9100";

export interface CreateArtworkOpts {
  artistSlug: string;
  title?: string;
  medium?: string;
  priceCents?: number;
  currency?: string;
  /** Free-form JSON matching `artworks.dimensions`. Omit to test
   * paths that key off missing dimensions (e.g. T-070 publish nudge). */
  dimensions?: Record<string, unknown> | null;
  /** "published" (default) or "draft". */
  status?: "published" | "draft";
  /** When true (default), also inserts an approved primary
   * `artwork_images` row so the artwork is publicly visible. */
  withImage?: boolean;
}

export interface CreateArtworkResp {
  id: string;
  image_id: string | null;
}

export interface CreateInquiryOpts {
  artworkId: string;
  fromName?: string;
  fromEmail?: string;
  message?: string;
  /** "delivered" (default) bumps the studio unread badge; "pending"
   * leaves `verified_at` + `delivered_at` NULL. */
  state?: "delivered" | "pending";
}

export interface CreateInquiryResp {
  id: string;
}

async function ctx() {
  return await playwrightRequest.newContext({ baseURL: API_URL });
}

/**
 * Insert an artwork under the given artist_slug. Optionally add an
 * approved primary image (default) so the artwork is publicly
 * visible + inquirable.
 */
export async function createArtwork(
  opts: CreateArtworkOpts,
): Promise<CreateArtworkResp> {
  const req = await ctx();
  try {
    const resp = await req.post("/v1/testfixtures/artwork", {
      data: {
        artist_slug: opts.artistSlug,
        title: opts.title,
        medium: opts.medium,
        price_cents: opts.priceCents,
        currency: opts.currency,
        dimensions: opts.dimensions,
        status: opts.status,
        with_image: opts.withImage,
      },
    });
    expect(
      resp.ok(),
      `createArtwork ${resp.status()} ${await resp.text()}`,
    ).toBeTruthy();
    return (await resp.json()) as CreateArtworkResp;
  } finally {
    await req.dispose();
  }
}

/**
 * Insert a delivered (default) or pending inquiry against an
 * artwork. Delivered inquiries bump the studio unread-badge count.
 */
export async function createInquiry(
  opts: CreateInquiryOpts,
): Promise<CreateInquiryResp> {
  const req = await ctx();
  try {
    const resp = await req.post("/v1/testfixtures/inquiry", {
      data: {
        artwork_id: opts.artworkId,
        from_name: opts.fromName,
        from_email: opts.fromEmail,
        message: opts.message,
        state: opts.state,
      },
    });
    expect(
      resp.ok(),
      `createInquiry ${resp.status()} ${await resp.text()}`,
    ).toBeTruthy();
    return (await resp.json()) as CreateInquiryResp;
  } finally {
    await req.dispose();
  }
}

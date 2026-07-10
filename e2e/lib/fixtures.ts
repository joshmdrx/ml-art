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
 * M-10 — flip an artist to charges/payouts-enabled without the real
 * Stripe Connect flow (the E2E stand-in for the `account.updated`
 * webhook). Makes their sellable works purchasable.
 */
export async function enablePayouts(artistSlug: string): Promise<void> {
  const req = await ctx();
  try {
    const resp = await req.post("/v1/testfixtures/enable-payouts", {
      data: { artist_slug: artistSlug },
    });
    expect(
      resp.ok(),
      `enablePayouts ${resp.status()} ${await resp.text()}`,
    ).toBeTruthy();
  } finally {
    await req.dispose();
  }
}

/**
 * M-10 — fill in the fields an artwork needs to be purchasable (weight,
 * ships-from, GBP price, dimensions). Pair with `enablePayouts` on the
 * owning artist so the Buy button shows.
 */
export async function makeSellable(artworkId: string): Promise<void> {
  const req = await ctx();
  try {
    const resp = await req.post("/v1/testfixtures/make-sellable", {
      data: { artwork_id: artworkId },
    });
    expect(
      resp.ok(),
      `makeSellable ${resp.status()} ${await resp.text()}`,
    ).toBeTruthy();
  } finally {
    await req.dispose();
  }
}

export interface CreateOrderOpts {
  /** Omit to attach any user as buyer (when identity doesn't matter). */
  buyerEmail?: string;
  /** Omit to attach any published artwork (dev DB isn't the test seed). */
  artworkId?: string;
  /** Any order status; defaults to `paid`. */
  status?: string;
  amountCentsGbp?: number;
}

/**
 * M-10 — insert an order in any state. The seam the buyer-orders,
 * mark-shipped, and admin-refund specs build on without the Stripe loop.
 */
export async function createOrder(
  opts: CreateOrderOpts,
): Promise<{ id: string }> {
  const req = await ctx();
  try {
    const resp = await req.post("/v1/testfixtures/order", {
      data: {
        buyer_email: opts.buyerEmail,
        artwork_id: opts.artworkId,
        status: opts.status,
        amount_cents_gbp: opts.amountCentsGbp,
      },
    });
    expect(
      resp.ok(),
      `createOrder ${resp.status()} ${await resp.text()}`,
    ).toBeTruthy();
    return (await resp.json()) as { id: string };
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

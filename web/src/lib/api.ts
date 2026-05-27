/**
 * Typed client for the Rust API.
 *
 * Mirrors `api/crates/core/src/models.rs`. When that file changes, update
 * here. (We'll move to `ts-rs` codegen later — for v0 hand-written is fine.)
 */

export type Availability = "available" | "sold" | "not_for_sale" | "inquire";

export type SortOrder =
  | "relevance"
  | "newest"
  | "price_asc"
  | "price_desc"
  | "nearest";

export interface ArtworkSummary {
  id: string;
  title: string | null;
  artist_name: string;
  artist_slug: string;
  primary_image_url: string | null;
  price_cents: number | null;
  currency: string;
  availability: Availability;
}

export interface Paginated<T> {
  items: T[];
  next_cursor: string | null;
}

export interface ArtistFull {
  id: string;
  slug: string;
  display_name: string;
  bio: string | null;
  artist_statement: string | null;
  location: string | null;
  city: string | null;
  country: string | null;
  lat: number | null;
  lng: number | null;
  website_url: string | null;
  socials: Record<string, string>;
  commissioning_preferences: unknown;
  representative_image_urls: string[];
}

export interface ArtistDetail {
  artist: ArtistFull;
  artworks: Paginated<ArtworkSummary>;
}

export interface Dimensions {
  height?: number;
  width?: number;
  depth?: number;
  unit?: "cm" | "in";
}

export interface ArtworkImage {
  id: string;
  url: string;
  width: number | null;
  height: number | null;
  is_primary: boolean;
  display_order: number;
}

export interface ArtworkArtist {
  id: string;
  slug: string;
  display_name: string;
  location: string | null;
}

export interface ArtworkFull {
  id: string;
  title: string | null;
  description: string | null;
  year_created: number | null;
  medium: string | null;
  dimensions: Dimensions | null;
  price_cents: number | null;
  currency: string;
  availability: Availability;
  external_url: string | null;
  published_at: string | null;
  artist: ArtworkArtist;
  images: ArtworkImage[];
}

export interface CollectionSummary {
  id: string;
  name: string;
  description: string | null;
  is_public: boolean;
  share_id: string | null;
  cover_image_urls: string[];
  artwork_count: number;
  updated_at: string;
  /**
   * True iff `listMyCollections` was called with an `artworkId` AND this
   * collection currently contains that artwork. Always `false` on plain
   * list calls. Used by the Save modal to render check-state without an
   * O(N) round of membership queries.
   */
  contains_artwork: boolean;
}

export interface CollectionDetail {
  collection: CollectionSummary;
  artworks: Paginated<ArtworkSummary>;
}

// ─────────────────────────────────────────────────────────────────────────────
// Studio — the authenticated artist's own surface (T-011)
// ─────────────────────────────────────────────────────────────────────────────

/** What `GET /v1/studio/me` returns. Used by the settings + portfolio pages. */
export interface StudioArtist {
  id: string;
  slug: string;
  display_name: string;
  bio: string | null;
  artist_statement: string | null;
  location: string | null;
  city: string | null;
  country: string | null;
  website_url: string | null;
  socials: Record<string, unknown>;
  commissioning_preferences: Record<string, unknown> | null;
  inquiry_preferences: Record<string, unknown>;
  /** `pending`, `active`, `paused`, `rejected`. Self-serve toggle only
   * accepts `active` ↔ `paused`. */
  status: "pending" | "active" | "paused" | "rejected";
  created_at: string;
  updated_at: string;
}

/** Body for `PATCH /v1/studio/settings`. Every field optional; omitting
 * a key means "don't touch." Use `null` to clear a nullable string. */
export interface StudioSettingsPatch {
  bio?: string | null;
  artist_statement?: string | null;
  location?: string | null;
  website_url?: string | null;
  socials?: Record<string, unknown>;
  commissioning_preferences?: Record<string, unknown> | null;
  inquiry_preferences?: Record<string, unknown>;
  status?: "active" | "paused";
}

/** Row in `GET /v1/studio/artworks`. Includes draft status (the public
 * artist endpoint hides drafts; studio surfaces them). */
export interface StudioArtworkSummary {
  id: string;
  title: string | null;
  status: "draft" | "published" | "archived";
  medium: string | null;
  price_cents: number | null;
  currency: string;
  availability: Availability;
  primary_image_url: string | null;
  created_at: string;
  updated_at: string;
  published_at: string | null;
}

/** Single artwork w/ images for the edit modal. */
export interface StudioArtworkDetail extends StudioArtworkSummary {
  description: string | null;
  year_created: number | null;
  dimensions: Dimensions | null;
  external_url: string | null;
  images: StudioImage[];
}

export interface StudioImage {
  id: string;
  s3_key: string;
  url: string;
  width: number | null;
  height: number | null;
  is_primary: boolean;
  display_order: number;
  moderation_status: "pending" | "approved" | "rejected";
}

/** Body for `POST /v1/studio/artworks`. All optional — the only thing
 * we mint a row for is the `artist_id` derived server-side. */
export interface CreateArtworkBody {
  title?: string;
  description?: string;
  year_created?: number;
  medium?: string;
  dimensions?: Dimensions;
  price_cents?: number;
  currency?: string;
  availability?: Availability;
  external_url?: string;
}

/** Body for `PATCH /v1/studio/artworks/:id`. `Option<Option<T>>` shape:
 * omit a key to leave it alone; pass `null` to clear a nullable field. */
export interface PatchArtworkBody {
  title?: string | null;
  description?: string | null;
  year_created?: number | null;
  medium?: string | null;
  dimensions?: Dimensions | null;
  price_cents?: number | null;
  currency?: string;
  availability?: Availability;
  external_url?: string | null;
  status?: "draft" | "published" | "archived";
}

export interface SearchParams {
  q?: string;
  medium?: string;
  price_min?: number;
  price_max?: number;
  availability?: Availability;
  location?: string;
  near_lat?: number;
  near_lng?: number;
  near_radius_km?: number;
  sort?: SortOrder;
  limit?: number;
}

const API_BASE =
  process.env.NEXT_PUBLIC_API_BASE_URL ?? "http://localhost:9100";

function toQueryString(params: SearchParams): string {
  const usp = new URLSearchParams();
  for (const [k, v] of Object.entries(params)) {
    if (v === undefined || v === null || v === "") continue;
    usp.set(k, String(v));
  }
  const s = usp.toString();
  return s ? `?${s}` : "";
}

/**
 * Server-side fetch helper. Forwards two identity headers to the Rust API
 * when available:
 *
 *   - `X-Anonymous-Id`: the unsigned UUID from the signed `anon_id` cookie
 *     (Next.js verifies the signature; the API gets the bare UUID)
 *   - `Authorization: Bearer <jwt>`: the signed-in user's Clerk session
 *     token, when one exists
 *
 * Both are best-effort: missing credentials are silently fine, the API has
 * extractors that handle "anonymous" and "signed-in" callers separately.
 *
 * Server-only — calling this from a client component throws because
 * `next/headers::cookies()` doesn't work in client contexts.
 */
async function apiFetch(
  path: string,
  init?: RequestInit
): Promise<Response> {
  // Lazy import so this module stays usable from non-Next test environments
  // (Vitest unit tests for the formatters etc.).
  const { cookies } = await import("next/headers");
  const { auth } = await import("@clerk/nextjs/server");
  const { ANON_COOKIE_NAME, verifyAnonId } = await import("./anonId");

  const headers = new Headers(init?.headers);

  // Anonymous-id header
  try {
    const jar = await cookies();
    const raw = jar.get(ANON_COOKIE_NAME)?.value;
    if (raw) {
      const uuid = await verifyAnonId(raw);
      if (uuid) headers.set("X-Anonymous-Id", uuid);
    }
  } catch {
    // `cookies()` throws outside a request context — that's fine, just
    // skip the header. Happens when this is called from a non-request
    // boundary (e.g. during build prerender).
  }

  // Clerk session JWT
  try {
    const { getToken } = await auth();
    const token = await getToken();
    if (token) headers.set("Authorization", `Bearer ${token}`);
  } catch {
    // No Clerk context (build-time, or no session) — skip the header.
  }

  return fetch(`${API_BASE}${path}`, { cache: "no-store", ...init, headers });
}

export async function searchArtworks(
  params: SearchParams,
  init?: RequestInit
): Promise<Paginated<ArtworkSummary>> {
  const res = await apiFetch(`/v1/search${toQueryString(params)}`, init);
  if (!res.ok) {
    const text = await res.text().catch(() => "");
    throw new Error(`search ${res.status}: ${text || res.statusText}`);
  }
  return (await res.json()) as Paginated<ArtworkSummary>;
}

export async function getArtist(
  slug: string,
  init?: RequestInit
): Promise<ArtistDetail | null> {
  const res = await apiFetch(`/v1/artists/${encodeURIComponent(slug)}`, init);
  if (res.status === 404) return null;
  if (!res.ok) {
    const text = await res.text().catch(() => "");
    throw new Error(`artist ${res.status}: ${text || res.statusText}`);
  }
  return (await res.json()) as ArtistDetail;
}

export interface Neighborhood {
  id: string;
  slug: string;
  name: string;
  description: string | null;
  kind: "curated" | "semantic" | "geographic";
  representative_image_urls: string[];
  artwork_count: number;
  is_featured: boolean;
}

export interface NeighborhoodDetail {
  neighborhood: Neighborhood;
  artworks: Paginated<ArtworkSummary>;
}

/** Subset of `SearchParams` the neighborhood detail endpoint accepts.
 * `location` is intentionally absent — the slug already pins place. */
export interface NeighborhoodFilters {
  medium?: string;
  price_min?: number;
  price_max?: number;
  availability?: string;
}

export async function listNeighborhoods(
  init?: RequestInit
): Promise<Paginated<Neighborhood>> {
  const res = await apiFetch("/v1/neighborhoods", init);
  if (!res.ok) {
    const text = await res.text().catch(() => "");
    throw new Error(`neighborhoods ${res.status}: ${text || res.statusText}`);
  }
  return (await res.json()) as Paginated<Neighborhood>;
}

export async function getNeighborhood(
  slug: string,
  filters?: NeighborhoodFilters,
  init?: RequestInit
): Promise<NeighborhoodDetail | null> {
  const usp = new URLSearchParams();
  if (filters?.medium) usp.set("medium", filters.medium);
  if (filters?.price_min !== undefined)
    usp.set("price_min", String(filters.price_min));
  if (filters?.price_max !== undefined)
    usp.set("price_max", String(filters.price_max));
  if (filters?.availability) usp.set("availability", filters.availability);
  const qs = usp.toString();
  const path = `/v1/neighborhoods/${encodeURIComponent(slug)}${qs ? `?${qs}` : ""}`;
  const res = await apiFetch(path, init);
  if (res.status === 404) return null;
  if (!res.ok) {
    const text = await res.text().catch(() => "");
    throw new Error(`neighborhood ${res.status}: ${text || res.statusText}`);
  }
  return (await res.json()) as NeighborhoodDetail;
}

export async function getArtwork(
  id: string,
  init?: RequestInit
): Promise<ArtworkFull | null> {
  const res = await apiFetch(`/v1/artworks/${encodeURIComponent(id)}`, init);
  if (res.status === 404) return null;
  if (!res.ok) {
    const text = await res.text().catch(() => "");
    throw new Error(`artwork ${res.status}: ${text || res.statusText}`);
  }
  return (await res.json()) as ArtworkFull;
}

export async function getSimilarArtworks(
  id: string,
  opts?: { limit?: number; include_same_artist?: boolean },
  init?: RequestInit
): Promise<Paginated<ArtworkSummary>> {
  const usp = new URLSearchParams();
  if (opts?.limit) usp.set("limit", String(opts.limit));
  if (opts?.include_same_artist)
    usp.set("include_same_artist", String(opts.include_same_artist));
  const qs = usp.toString() ? `?${usp.toString()}` : "";
  const res = await apiFetch(
    `/v1/artworks/${encodeURIComponent(id)}/similar${qs}`,
    init
  );
  if (!res.ok) {
    const text = await res.text().catch(() => "");
    throw new Error(`similar ${res.status}: ${text || res.statusText}`);
  }
  return (await res.json()) as Paginated<ArtworkSummary>;
}

// ─────────────────────────────────────────────────────────────────────────────
// Inquiries
// ─────────────────────────────────────────────────────────────────────────────

export interface InquiryAck {
  id: string;
  /** "delivered" | "pending_verification" */
  status: string;
  /** Dev-mode helper. Won't be present in prod. */
  debug_verification_token?: string;
}

export async function submitInquiry(
  artworkId: string,
  input: {
    name: string;
    email?: string;
    message: string;
    budget_range?: string;
  }
): Promise<InquiryAck> {
  const res = await apiFetch(
    `/v1/artworks/${encodeURIComponent(artworkId)}/inquiries`,
    {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify(input),
    }
  );
  if (!res.ok) {
    const text = await res.text().catch(() => "");
    throw new Error(`inquiry ${res.status}: ${text || res.statusText}`);
  }
  return (await res.json()) as InquiryAck;
}

export async function verifyInquiry(
  token: string
): Promise<{ status: string } | null> {
  const res = await apiFetch(
    `/v1/inquiries/verify/${encodeURIComponent(token)}`
  );
  if (res.status === 404) return null;
  if (!res.ok) {
    const text = await res.text().catch(() => "");
    throw new Error(`verify ${res.status}: ${text || res.statusText}`);
  }
  return (await res.json()) as { status: string };
}

// ─────────────────────────────────────────────────────────────────────────────
// Collections (authed)
// ─────────────────────────────────────────────────────────────────────────────

export async function listMyCollections(
  opts?: { artworkId?: string; init?: RequestInit }
): Promise<Paginated<CollectionSummary>> {
  // When the caller passes `artworkId`, each returned summary's
  // `contains_artwork` reflects current membership. Used by the Save
  // modal to render check-state in one round-trip.
  const path = opts?.artworkId
    ? `/v1/me/collections?artwork_id=${encodeURIComponent(opts.artworkId)}`
    : "/v1/me/collections";
  const res = await apiFetch(path, opts?.init);
  if (res.status === 401) {
    return { items: [], next_cursor: null };
  }
  if (!res.ok) {
    const text = await res.text().catch(() => "");
    throw new Error(`collections ${res.status}: ${text || res.statusText}`);
  }
  return (await res.json()) as Paginated<CollectionSummary>;
}

export async function getCollection(
  id: string,
  init?: RequestInit
): Promise<CollectionDetail | null> {
  const res = await apiFetch(
    `/v1/me/collections/${encodeURIComponent(id)}`,
    init
  );
  if (res.status === 404 || res.status === 401) return null;
  if (!res.ok) {
    const text = await res.text().catch(() => "");
    throw new Error(`collection ${res.status}: ${text || res.statusText}`);
  }
  return (await res.json()) as CollectionDetail;
}

export async function createCollection(input: {
  name: string;
  description?: string;
  is_public?: boolean;
}): Promise<CollectionSummary> {
  const res = await apiFetch("/v1/me/collections", {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify(input),
  });
  if (!res.ok) {
    const text = await res.text().catch(() => "");
    throw new Error(`create collection ${res.status}: ${text || res.statusText}`);
  }
  return (await res.json()) as CollectionSummary;
}

export async function patchCollection(
  id: string,
  input: { name?: string; description?: string | null; is_public?: boolean }
): Promise<CollectionSummary> {
  const res = await apiFetch(`/v1/me/collections/${encodeURIComponent(id)}`, {
    method: "PATCH",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify(input),
  });
  if (!res.ok) {
    const text = await res.text().catch(() => "");
    throw new Error(`patch collection ${res.status}: ${text || res.statusText}`);
  }
  return (await res.json()) as CollectionSummary;
}

export async function deleteCollection(id: string): Promise<void> {
  const res = await apiFetch(`/v1/me/collections/${encodeURIComponent(id)}`, {
    method: "DELETE",
  });
  if (!res.ok && res.status !== 404) {
    const text = await res.text().catch(() => "");
    throw new Error(`delete collection ${res.status}: ${text || res.statusText}`);
  }
}

export async function addArtworkToCollection(
  collectionId: string,
  artworkId: string
): Promise<void> {
  const res = await apiFetch(
    `/v1/me/collections/${encodeURIComponent(collectionId)}/artworks`,
    {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ artwork_id: artworkId }),
    }
  );
  if (!res.ok) {
    const text = await res.text().catch(() => "");
    throw new Error(`add artwork ${res.status}: ${text || res.statusText}`);
  }
}

export async function removeArtworkFromCollection(
  collectionId: string,
  artworkId: string
): Promise<void> {
  const res = await apiFetch(
    `/v1/me/collections/${encodeURIComponent(collectionId)}/artworks/${encodeURIComponent(artworkId)}`,
    { method: "DELETE" }
  );
  if (!res.ok && res.status !== 404) {
    const text = await res.text().catch(() => "");
    throw new Error(`remove artwork ${res.status}: ${text || res.statusText}`);
  }
}

export function formatDimensions(d: Dimensions | null): string | null {
  if (!d || (d.height == null && d.width == null)) return null;
  const unit = d.unit ?? "cm";
  const parts = [d.height, d.width, d.depth]
    .filter((n): n is number => typeof n === "number")
    .map((n) => `${n}`);
  if (parts.length === 0) return null;
  return `${parts.join(" × ")} ${unit}`;
}

export function formatPrice(
  cents: number | null,
  currency: string
): string | null {
  if (cents === null) return null;
  const major = cents / 100;
  try {
    return new Intl.NumberFormat("en-US", {
      style: "currency",
      currency,
      maximumFractionDigits: 0,
    }).format(major);
  } catch {
    return `${major.toFixed(0)} ${currency}`;
  }
}

// ─────────────────────────────────────────────────────────────────────────────
// Studio (authed; artist-only)
// ─────────────────────────────────────────────────────────────────────────────

/** Fetch the artist record for the current Clerk user. Returns `null` if
 * the user isn't signed in (401) or has no artist row (404 from
 * `current_artist_id`) — both cases collapse to "not an artist (yet)"
 * from the UI's perspective. */
export async function getStudioMe(
  init?: RequestInit
): Promise<StudioArtist | null> {
  const res = await apiFetch("/v1/studio/me", init);
  if (res.status === 401 || res.status === 404) return null;
  if (!res.ok) {
    const text = await res.text().catch(() => "");
    throw new Error(`studio/me ${res.status}: ${text || res.statusText}`);
  }
  return (await res.json()) as StudioArtist;
}

/** Patch the current artist's settings. Body keys are optional; only
 * include the fields you intend to change. Returns the updated artist. */
export async function updateStudioSettings(
  body: StudioSettingsPatch,
  init?: RequestInit
): Promise<StudioArtist> {
  const res = await apiFetch("/v1/studio/settings", {
    ...init,
    method: "PATCH",
    headers: { "Content-Type": "application/json", ...(init?.headers ?? {}) },
    body: JSON.stringify(body),
  });
  if (!res.ok) {
    const text = await res.text().catch(() => "");
    throw new Error(`studio/settings ${res.status}: ${text || res.statusText}`);
  }
  return (await res.json()) as StudioArtist;
}

/** List the current artist's artworks. `null` means "not an artist" — same
 * collapsed-failure pattern as `getStudioMe()`. */
export async function listMyArtworks(
  opts?: { status?: "draft" | "published" | "archived" | "all"; init?: RequestInit }
): Promise<Paginated<StudioArtworkSummary> | null> {
  const usp = new URLSearchParams();
  if (opts?.status) usp.set("status", opts.status);
  const qs = usp.toString();
  const res = await apiFetch(
    `/v1/studio/artworks${qs ? `?${qs}` : ""}`,
    opts?.init
  );
  if (res.status === 401 || res.status === 404) return null;
  if (!res.ok) {
    const text = await res.text().catch(() => "");
    throw new Error(`studio/artworks ${res.status}: ${text || res.statusText}`);
  }
  return (await res.json()) as Paginated<StudioArtworkSummary>;
}

/** Studio detail view of a single artwork (includes drafts + images). */
export async function getStudioArtwork(
  id: string,
  init?: RequestInit
): Promise<StudioArtworkDetail | null> {
  const res = await apiFetch(
    `/v1/studio/artworks/${encodeURIComponent(id)}`,
    init
  );
  if (res.status === 401 || res.status === 404) return null;
  if (!res.ok) {
    const text = await res.text().catch(() => "");
    throw new Error(`studio/artworks/:id ${res.status}: ${text || res.statusText}`);
  }
  return (await res.json()) as StudioArtworkDetail;
}

export async function createStudioArtwork(
  body: CreateArtworkBody
): Promise<StudioArtworkSummary> {
  const res = await apiFetch("/v1/studio/artworks", {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify(body),
  });
  if (!res.ok) {
    const text = await res.text().catch(() => "");
    throw new Error(`studio/artworks ${res.status}: ${text || res.statusText}`);
  }
  return (await res.json()) as StudioArtworkSummary;
}

export async function patchStudioArtwork(
  id: string,
  body: PatchArtworkBody
): Promise<StudioArtworkSummary> {
  const res = await apiFetch(`/v1/studio/artworks/${encodeURIComponent(id)}`, {
    method: "PATCH",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify(body),
  });
  if (!res.ok) {
    const text = await res.text().catch(() => "");
    throw new Error(`studio/artworks/:id ${res.status}: ${text || res.statusText}`);
  }
  return (await res.json()) as StudioArtworkSummary;
}

export async function deleteStudioArtwork(id: string): Promise<void> {
  const res = await apiFetch(`/v1/studio/artworks/${encodeURIComponent(id)}`, {
    method: "DELETE",
  });
  if (res.status === 204) return;
  if (!res.ok) {
    const text = await res.text().catch(() => "");
    throw new Error(`studio/artworks/:id ${res.status}: ${text || res.statusText}`);
  }
}

export async function addStudioArtworkImage(
  id: string,
  body: { s3_key: string; is_primary?: boolean; width?: number; height?: number }
): Promise<StudioImage> {
  const res = await apiFetch(
    `/v1/studio/artworks/${encodeURIComponent(id)}/images`,
    {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify(body),
    }
  );
  if (!res.ok) {
    const text = await res.text().catch(() => "");
    throw new Error(
      `studio/artworks/:id/images ${res.status}: ${text || res.statusText}`
    );
  }
  return (await res.json()) as StudioImage;
}

export async function removeStudioArtworkImage(
  artworkId: string,
  imageId: string
): Promise<void> {
  const res = await apiFetch(
    `/v1/studio/artworks/${encodeURIComponent(artworkId)}/images/${encodeURIComponent(imageId)}`,
    { method: "DELETE" }
  );
  if (res.status === 204) return;
  if (!res.ok) {
    const text = await res.text().catch(() => "");
    throw new Error(
      `studio/artworks/:id/images ${res.status}: ${text || res.statusText}`
    );
  }
}

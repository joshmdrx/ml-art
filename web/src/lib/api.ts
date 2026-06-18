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
  /** Stable artist id. Powers the "Where to see them" map view: the
   * search page collects the distinct artist ids from a grid result
   * and passes them to /v1/search/map?artist_ids=… so the map mirrors
   * the grid without re-running the embedding. */
  artist_id: string;
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
  /** Public locations — only rows where the geocode has landed
   * (`lat`/`lng` are guaranteed non-null on this surface; the API
   * filters out pre-geocode rows). Empty list when the artist has none.
   * Optional on the type so callers don't crash when an older API
   * build (pre-T-038) omits the field. T-038. */
  locations?: PublicArtistLocation[];
}

/** Lighter mirror of `StudioLocation` for public surfaces — same wire
 * shape, but lat/lng are guaranteed non-null by the API. */
export interface PublicArtistLocation {
  id: string;
  kind: "gallery" | "studio";
  name: string;
  address: string;
  city: string | null;
  country: string | null;
  lat: number;
  lng: number;
  website_url: string | null;
  display_order: number;
}

/** A pin returned by `/v1/search/map` (T-038 G5). One row per
 * `artist_locations` row matching the active filters, with a small
 * artist + thumb payload baked in for the popover. */
export interface MapPin {
  location_id: string;
  lat: number;
  lng: number;
  name: string;
  kind: "gallery" | "studio";
  city: string | null;
  country: string | null;
  artist: {
    slug: string;
    display_name: string;
    primary_image_url: string | null;
  };
}

/** A city pivot returned by `/v1/search/map/cities` (T-042). Powers
 * the city-pill strip on `/search?map=1`. `west/south/east/north` is
 * the tight bbox of every pin in that city — degenerate to a point
 * when there's only one pin, expands as more land. */
export interface CityPivot {
  city: string;
  country: string | null;
  count: number;
  center_lat: number;
  center_lng: number;
  west: number;
  south: number;
  east: number;
  north: number;
}

/** Params for `/v1/search/map`. Subset of `SearchParams` — only the
 * filters that make sense for picking *where to go* (vs ranking
 * artworks). See `api-search::search_map` for the rationale. */
export interface MapSearchParams {
  q?: string;
  medium?: string;
  location?: string;
  /** "west,south,east,north" (lng,lat,lng,lat). Mapbox bounds are
   * available as `.toArray().flat().join(',')`. */
  bbox?: string;
  /** Pin down to a single artist by slug (T-041). Set by the
   * "See on map" CTA on `/artists/[slug]`. */
  artist?: string;
  /** "uuid1,uuid2,…" — the "map = view of grid result" path. When
   * set, the map shows venues for exactly these artists. Used by
   * /search?map=1 to keep map + grid consistent: the search page
   * collects ArtworkSummary.artist_id from the grid response and
   * passes them here. */
  artist_ids?: string;
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
  /** Comma-joined labels persisted on rejection (e.g.
   * "Explicit Nudity, Suggestive"). `null` for pending +
   * approved rows. Surfaced in studio so the artist can see why
   * an image is hidden from public surfaces. T-008c. */
  moderation_reason: string | null;
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

/** A row in `GET /v1/studio/inquiries` (T-011 Phase 4). The artist's
 * inquiry inbox — read-only. `status` is derived from `delivered_at`
 * server-side: `"delivered"` for anything that's been sent to the
 * artist (signed-in inquiries land here immediately; anonymous after
 * the inquirer clicks the verification link), `"pending_verification"`
 * while we're still waiting on the anon round-trip. */
export interface StudioInquiry {
  id: string;
  artwork_id: string;
  artwork_title: string | null;
  artwork_primary_image_url: string | null;
  from_name: string;
  from_email: string;
  message: string;
  budget_range: string | null;
  status: "delivered" | "pending_verification";
  created_at: string;
  delivered_at: string | null;
  /** When the artist last opened this row. `null` ≡ unread. T-011 Phase 4b. */
  read_at: string | null;
  /** Artist's outgoing replies, oldest first. T-011 Phase 4b. */
  replies: StudioInquiryReply[];
}

/** One artist reply on a `StudioInquiry`. `sent_at` is set by the email
 * handler once Resend confirms the send; null while in-flight. */
export interface StudioInquiryReply {
  id: string;
  message: string;
  created_at: string;
  sent_at: string | null;
}

// Studio locations (T-038 G3) — "Where to see my work" CRUD.

/** A row in `GET /v1/studio/locations`. Mirrors the public `ArtistLocation`
 * but the studio surface includes pre-geocode rows (`lat`/`lng` null),
 * which the public artist profile hides. */
export interface StudioLocation {
  id: string;
  kind: "gallery" | "studio";
  name: string;
  address: string;
  city: string | null;
  country: string | null;
  lat: number | null;
  lng: number | null;
  website_url: string | null;
  display_order: number;
  geocoded_at: string | null;
}

export interface CreateLocationBody {
  kind: "gallery" | "studio";
  name: string;
  address: string;
  website_url?: string;
  display_order?: number;
}

/** PATCH body for `/v1/studio/locations/:id`. Omit a key to leave it
 * alone; pass `null` for `website_url` to clear it. */
export interface PatchLocationBody {
  kind?: "gallery" | "studio";
  name?: string;
  address?: string;
  website_url?: string | null;
  display_order?: number;
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
  /** Visual-search anchor — overrides `q`'s text vector for the
   * semantic side. T-010 Phase B. */
  image_upload_id?: string;
  /** Visual-search anchor sourced from an *existing platform artwork*
   * (vs an uploaded image). Server resolves the artwork's embedding
   * directly from `artwork_embeddings` — no upload roundtrip.
   * Precedence: `image_upload_id` > `seed_artwork_id` > `q` text
   * embed. The seed artwork itself is excluded from results. */
  seed_artwork_id?: string;
  /** Comma-separated modifier names (`moodier,warmer,…`). Each
   * shifts the anchor along its δ-vector at α=0.8 server-side.
   * Requires `image_upload_id` *or* `seed_artwork_id`. T-010 Phase C. */
  modifiers?: string;
  /** Opaque cursor from a prior response's `next_cursor`. T-037. */
  cursor?: string;
}

/** Returned by `GET /v1/modifiers` for the search-page button row. */
export interface SearchModifier {
  name: string;
  label: string;
}

/** Acknowledgement from `POST /v1/uploads/image`. */
export interface UploadAck {
  upload_id: string;
  s3_key: string;
  image_url: string;
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

/** T-042 — fetch the top-cities pivot list for `/search?map=1`'s
 * city-pill strip. Empty array on success-with-no-cities (the
 * cold-start case before any artist has geocoded locations). */
export async function listMapCities(
  filters?: { q?: string; medium?: string; artist_ids?: string[] },
  init?: RequestInit
): Promise<CityPivot[]> {
  // Forward the active filters so the pivot strip mirrors what the
  // map underneath it actually shows. `artist_ids` (when present)
  // wins: it carries the upstream grid's RRF/vector result set, so
  // the strip + map both restrict to those artists.
  const usp = new URLSearchParams();
  if (filters?.q?.trim()) usp.set("q", filters.q.trim());
  if (filters?.medium?.trim()) usp.set("medium", filters.medium.trim());
  if (filters?.artist_ids && filters.artist_ids.length > 0) {
    usp.set("artist_ids", filters.artist_ids.join(","));
  }
  const qs = usp.toString();
  const res = await apiFetch(
    `/v1/search/map/cities${qs ? `?${qs}` : ""}`,
    init
  );
  if (!res.ok) {
    const text = await res.text().catch(() => "");
    throw new Error(
      `search/map/cities ${res.status}: ${text || res.statusText}`
    );
  }
  return (await res.json()) as CityPivot[];
}

/** T-038 G5 — fetch map pins matching the given filters. Returns an
 * empty array (not null) on success-with-no-results so the map page
 * can render its empty state cleanly. */
export async function searchMap(
  params: MapSearchParams,
  init?: RequestInit
): Promise<MapPin[]> {
  const res = await apiFetch(`/v1/search/map${toQueryString(params)}`, init);
  if (!res.ok) {
    const text = await res.text().catch(() => "");
    throw new Error(`search/map ${res.status}: ${text || res.statusText}`);
  }
  return (await res.json()) as MapPin[];
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

/** T-053 — public read of a collection by its share token. Unauthenticated.
 *  Returns null for 404 (no row, private, or deleted) — the API does not
 *  distinguish these cases. */
export async function getPublicCollection(
  shareId: string,
  init?: RequestInit
): Promise<CollectionDetail | null> {
  const res = await apiFetch(
    `/v1/collections/share/${encodeURIComponent(shareId)}`,
    init
  );
  if (res.status === 404) return null;
  if (!res.ok) {
    const text = await res.text().catch(() => "");
    throw new Error(
      `public collection ${res.status}: ${text || res.statusText}`
    );
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

// Formatters moved to `@/lib/format` so client components can call
// them without dragging `apiFetch`'s server-only Clerk import into
// the client bundle. Re-exported here for backward compatibility
// with existing callers (artworks/[id]/page.tsx, format.test.ts).
export { formatDimensions, formatPrice } from "@/lib/format";

// ─────────────────────────────────────────────────────────────────────────────
// Studio (authed; artist-only)
// ─────────────────────────────────────────────────────────────────────────────

/** Fetch the artist record for the current Clerk user. Returns `null` if
 * the user isn't signed in (401) or has no artist row (404 from
 * `current_artist_id`) — both cases collapse to "not an artist (yet)"
 * from the UI's perspective. */
// ─────────────────────────────────────────────────────────────────────────────
// Onboarding (T-012 Phase 1) — mints an artist row + flips it to active.
// ─────────────────────────────────────────────────────────────────────────────

/** Body for `POST /v1/onboarding/start`. */
export interface StartOnboardingBody {
  display_name: string;
  location?: string;
}

/** `POST /v1/onboarding/start` — creates the caller's `artists` row with
 * `status='pending'` and links `user_id`. Returns the new `StudioArtist`.
 * Throws if the API errors (already-onboarded, validation, 401). */
export async function startOnboarding(
  body: StartOnboardingBody
): Promise<StudioArtist> {
  const res = await apiFetch("/v1/onboarding/start", {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify(body),
  });
  if (!res.ok) {
    const text = await res.text().catch(() => "");
    throw new Error(`onboarding/start ${res.status}: ${text || res.statusText}`);
  }
  return (await res.json()) as StudioArtist;
}

/** `POST /v1/onboarding/complete` — flips `status: pending → active`.
 * Idempotent on already-active artists. */
export async function completeOnboarding(): Promise<StudioArtist> {
  const res = await apiFetch("/v1/onboarding/complete", { method: "POST" });
  if (!res.ok) {
    const text = await res.text().catch(() => "");
    throw new Error(
      `onboarding/complete ${res.status}: ${text || res.statusText}`
    );
  }
  return (await res.json()) as StudioArtist;
}

/** T-033 — copy behavioral signal keyed on the caller's anon_id over
 * to their now-known user_id. Idempotent: a second call is a no-op
 * because the WHERE clause server-side filters on `user_id IS NULL`.
 * Returns the counts so callers can log "merged N uploads". */
export interface MergeAnonymousResponse {
  uploads_merged: number;
  events_merged: number;
}

export async function mergeAnonymous(
  init?: RequestInit
): Promise<MergeAnonymousResponse> {
  const res = await apiFetch("/v1/me/merge-anonymous", {
    ...init,
    method: "POST",
  });
  if (!res.ok) {
    const text = await res.text().catch(() => "");
    throw new Error(
      `me/merge-anonymous ${res.status}: ${text || res.statusText}`
    );
  }
  return (await res.json()) as MergeAnonymousResponse;
}

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

/** List the calling artist's inquiry inbox (T-011 Phase 4). `null`
 * means "not an artist" — same collapsed-failure pattern as
 * `getStudioMe()`. */
export async function listStudioInquiries(opts?: {
  status?: "pending" | "delivered" | "all";
  init?: RequestInit;
}): Promise<Paginated<StudioInquiry> | null> {
  const usp = new URLSearchParams();
  if (opts?.status && opts.status !== "all") usp.set("status", opts.status);
  const qs = usp.toString();
  const res = await apiFetch(
    `/v1/studio/inquiries${qs ? `?${qs}` : ""}`,
    opts?.init
  );
  if (res.status === 401 || res.status === 404) return null;
  if (!res.ok) {
    const text = await res.text().catch(() => "");
    throw new Error(
      `studio/inquiries ${res.status}: ${text || res.statusText}`
    );
  }
  return (await res.json()) as Paginated<StudioInquiry>;
}

/** Send an artist reply on one inquiry (T-011 Phase 4b). Server
 * persists the row + enqueues a Resend send job. Returns the
 * newly-created reply (with `sent_at: null` until the worker runs). */
export async function postStudioInquiryReply(
  inquiryId: string,
  message: string,
): Promise<StudioInquiryReply> {
  const res = await apiFetch(
    `/v1/studio/inquiries/${encodeURIComponent(inquiryId)}/reply`,
    {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({ message }),
    },
  );
  if (!res.ok) {
    const text = await res.text().catch(() => "");
    throw new Error(
      `studio/inquiries/reply ${res.status}: ${text || res.statusText}`,
    );
  }
  return (await res.json()) as StudioInquiryReply;
}

/** Bulk mark-as-read on inbox view (T-011 Phase 4b). Returns the
 * count actually flipped — ignored ids (other artists', already
 * read) silently drop out of the count without erroring. */
export async function markStudioInquiriesRead(
  ids: string[],
): Promise<{ updated: number }> {
  const res = await apiFetch(`/v1/studio/inquiries/read`, {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify({ ids }),
  });
  if (!res.ok) {
    const text = await res.text().catch(() => "");
    throw new Error(
      `studio/inquiries/read ${res.status}: ${text || res.statusText}`,
    );
  }
  return (await res.json()) as { updated: number };
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

// Studio locations (T-038 G3) — CRUD for "Where to see my work."

export async function listStudioLocations(
  init?: RequestInit
): Promise<StudioLocation[] | null> {
  const res = await apiFetch("/v1/studio/locations", init);
  if (res.status === 401 || res.status === 404) return null;
  if (!res.ok) {
    const text = await res.text().catch(() => "");
    throw new Error(
      `studio/locations ${res.status}: ${text || res.statusText}`
    );
  }
  return (await res.json()) as StudioLocation[];
}

export async function createStudioLocation(
  body: CreateLocationBody
): Promise<StudioLocation> {
  const res = await apiFetch("/v1/studio/locations", {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify(body),
  });
  if (!res.ok) {
    const text = await res.text().catch(() => "");
    throw new Error(
      `studio/locations POST ${res.status}: ${text || res.statusText}`
    );
  }
  return (await res.json()) as StudioLocation;
}

export async function patchStudioLocation(
  id: string,
  body: PatchLocationBody
): Promise<StudioLocation> {
  const res = await apiFetch(`/v1/studio/locations/${encodeURIComponent(id)}`, {
    method: "PATCH",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify(body),
  });
  if (!res.ok) {
    const text = await res.text().catch(() => "");
    throw new Error(
      `studio/locations PATCH ${res.status}: ${text || res.statusText}`
    );
  }
  return (await res.json()) as StudioLocation;
}

export async function deleteStudioLocation(id: string): Promise<void> {
  const res = await apiFetch(
    `/v1/studio/locations/${encodeURIComponent(id)}`,
    { method: "DELETE" }
  );
  if (!res.ok) {
    const text = await res.text().catch(() => "");
    throw new Error(
      `studio/locations DELETE ${res.status}: ${text || res.statusText}`
    );
  }
}

// ─────────────────────────────────────────────────────────────────────────────
// Visual search — uploads + modifier list (T-010 Phase D)
// ─────────────────────────────────────────────────────────────────────────────

/** `GET /v1/modifiers` — static registry the search-page button row
 * iterates over. Returns `[]` if the call fails (page still renders
 * with no modifier row). */
export async function listSearchModifiers(
  init?: RequestInit
): Promise<SearchModifier[]> {
  const res = await apiFetch("/v1/modifiers", init);
  if (!res.ok) {
    const text = await res.text().catch(() => "");
    throw new Error(`modifiers ${res.status}: ${text || res.statusText}`);
  }
  return (await res.json()) as SearchModifier[];
}

/** `POST /v1/uploads/image` — multipart upload. Server-side only; the
 * Bearer / anon-id forwarding lives in `apiFetch`. */
export async function uploadImageForSearch(
  file: { name: string; type: string; bytes: Uint8Array }
): Promise<UploadAck> {
  // Build a multipart body by hand. The browser would normally do this
  // for us, but server actions hand us `File` -> we re-serialize before
  // forwarding to the Rust API.
  const boundary = `----wander-${crypto.randomUUID()}`;
  const enc = new TextEncoder();
  const head = enc.encode(
    `--${boundary}\r\n` +
      `Content-Disposition: form-data; name="image"; filename="${file.name}"\r\n` +
      `Content-Type: ${file.type}\r\n\r\n`
  );
  const tail = enc.encode(`\r\n--${boundary}--\r\n`);
  const body = new Uint8Array(head.length + file.bytes.length + tail.length);
  body.set(head, 0);
  body.set(file.bytes, head.length);
  body.set(tail, head.length + file.bytes.length);

  const res = await apiFetch("/v1/uploads/image", {
    method: "POST",
    headers: { "Content-Type": `multipart/form-data; boundary=${boundary}` },
    body,
  });
  if (!res.ok) {
    const text = await res.text().catch(() => "");
    throw new Error(`uploads/image ${res.status}: ${text || res.statusText}`);
  }
  return (await res.json()) as UploadAck;
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

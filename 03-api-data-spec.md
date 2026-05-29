# Art Discovery Platform — API + Data Spec (v1)

> **Aspirational.** Describes the intended v1 API surface, not
> necessarily what's in the code today. Truth for shipped endpoints
> lives in [`CHANGELOG.md`](CHANGELOG.md) + the Rust source itself. See
> `decisions.md` 2026-05-27 — Specs are aspirational, CHANGELOG +
> decisions are truth.

## Principles

- REST-ish over HTTPS. JSON in/out. No GraphQL.
- Rust types as source of truth; TS types generated via `ts-rs` for the client. Zod schemas on the client mirror them for runtime validation at boundaries.
- Auth via Clerk JWTs in `Authorization: Bearer` header. API validates JWT, extracts `user_id`.
- Anonymous identity carried by a **signed, HTTP-only first-party cookie** (`anon_id`), set by Next.js middleware on first request. Never trust a client-supplied anonymous_id header or body field.
- Every endpoint instrumented — request logged, event emitted on meaningful actions.
- Rate-limited per-IP and per-identity via Upstash Redis (`@upstash/ratelimit`).
- Cursor-based pagination (not offset).
- Versioned under `/v1/` from day one.

## Auth model

- Clerk handles all auth UI and session management.
- API validates Clerk JWT on requests requiring auth.
- Anonymous identity: signed HMAC cookie `anon_id` (UUID v7), set by Next.js edge middleware on first request, HTTP-only, SameSite=Lax, 1-year expiry. API extracts and verifies the signature server-side; the unsigned UUID is what gets logged/joined to events.
- On sign-in/up, client calls `POST /v1/me/merge-anonymous` (no body). Server reads the `anon_id` cookie, merges its behavioral data into the authed user account, then optionally rotates the cookie. Idempotent.

## Rate limiting

| Endpoint | Limit | Key |
|---|---|---|
| `POST /v1/artworks/:id/inquiries` | 3 / hour | IP + anon_id |
| `POST /v1/uploads/image` | 20 / hour | IP + anon_id |
| `GET /v1/search` | 60 / minute | anon_id |
| `POST /v1/events` | 200 / minute | anon_id |
| All other write endpoints | 30 / minute | user_id or IP |

Implemented as middleware in the Rust API using a Redis-backed sliding window. 429 response uses RFC 7807 problem+json with `Retry-After` header.

## Cookie consent

EU visitors see a one-time consent banner before any non-essential cookies are set (PostHog identifier, etc.). Until consent is granted, events are buffered client-side and only flushed on accept. The `anon_id` cookie is treated as strictly-necessary (no consent required) since it underpins core anti-abuse and session continuity; this is documented in the privacy policy.

## Conventions

- All IDs are UUIDs (v7 for time-sortable).
- Timestamps: ISO 8601 UTC strings on the wire, `timestamptz` in Postgres.
- Money: `price_cents` integer + `currency` ISO code.
- Errors: RFC 7807 problem+json format. `{type, title, status, detail, instance}`.
- Paginated lists: `{ items: T[], next_cursor: string | null }`.
- Soft-delete: `deleted_at` column; default queries filter it out.

---

## Endpoints

### Public discovery

#### `GET /v1/search`

Query params:
- `q` (string, optional) — text query.
- `image_upload_id` (uuid, optional) — reference to an uploaded image for visual search.
- `modifiers` (comma-separated, optional) — e.g. `moodier,warmer`.
- `medium` (array).
- `price_min`, `price_max` (integers, cents).
- `size_min`, `size_max` (integers, cm — longest dimension).
- `orientation` (portrait|landscape|square).
- `availability` (available|all).
- `color` (hex, optional) — nearest-color match.
- `location` (string, optional) — loose match against artist `city` or `country`. ILIKE on a normalized "city, country" string. e.g. `?location=berlin` matches "Berlin", "Berlin, Germany". One location term per query.
- `near_lat`, `near_lng`, `near_radius_km` (optional, all three or none) — proximity filter. Computes great-circle distance via Haversine in SQL. Default radius 50km if `near_lat/lng` are set without `radius`.
- `sort` (relevance|newest|price_asc|price_desc|nearest). `nearest` only valid with `near_lat/lng`.
- `cursor` (opaque).
- `limit` (default 24, max 48).

Response: `{ items: ArtworkSummary[], next_cursor, facet_counts: { medium: {...}, ... } }`.

Behavior: runs the hybrid ranking pipeline. Logs `search_executed` event with query and filter state.

#### `GET /v1/artworks/:id`

Response: full artwork with artist summary, images, metadata, tags.

Side effect: logs `artwork_viewed` event.

#### `GET /v1/artworks/:id/similar`

Query: `limit` (default 8).

Response: `{ items: ArtworkSummary[] }`.

Behavior: k-NN on embedding, excluding same artist's other works unless specifically requested (`include_same_artist=true`).

#### `GET /v1/artists/:slug`

Response: artist profile with first page of artworks.

#### `GET /v1/artists/:slug/artworks`

Query: `cursor`, `limit`, `status_filter` (available|all).

Response: paginated artwork list.

#### `GET /v1/artists/:slug/similar`

Response: `{ items: ArtistSummary[] }`. Similar artists (6).

#### `GET /v1/neighborhoods`

Response: `{ items: NeighborhoodSummary[] }`. All neighborhoods with representative thumbnails.

#### `GET /v1/neighborhoods/:slug`

Response: neighborhood metadata + first page of artworks (accepts same filters as search).

#### `GET /v1/neighborhoods/:slug/artworks`

Paginated list with filter support. Same filter shape as search.

### Visual search

#### `POST /v1/uploads/image`

Body: multipart form-data with `file`.

Response: `{ upload_id, preview_url }`.

Behavior:
1. Validates file (type, size, dimensions).
2. Stores in S3 under `uploads/` prefix.
3. Enqueues two Inngest jobs in parallel: `image.moderate` (AWS Rekognition `DetectModerationLabels`) and `image.embed` (call Jina/Voyage).
4. Response returns immediately with `upload_id` and a `preview_url`.
5. If moderation rejects, the upload is marked `rejected` and any subsequent search using it returns a 422.
6. Embedding is generated on-upload (not lazy) so the first search using this upload doesn't pay the embedding-API roundtrip in the user's critical path.
7. Uploads auto-expire after 24h if unused.

### Collections (authed)

#### `GET /v1/me/collections`

Response: `{ items: Collection[] }`.

#### `POST /v1/me/collections`

Body: `{ name, description?, is_public? }`.

Response: created collection.

#### `GET /v1/me/collections/:id`

Response: collection with artworks.

#### `PATCH /v1/me/collections/:id`

Body: `{ name?, description?, is_public? }`.

#### `DELETE /v1/me/collections/:id`

Soft-delete.

#### `POST /v1/me/collections/:id/artworks`

Body: `{ artwork_id }`.

Response: `{ collection_id, artwork_id, added_at }`.

Logs `artwork_saved` event.

#### `DELETE /v1/me/collections/:id/artworks/:artwork_id`

Removes artwork from collection.

### Public shared collections

#### `GET /v1/collections/shared/:share_id`

Public read. No auth required. Read-only view.

### User

#### `GET /v1/me`

Response: user profile.

#### `PATCH /v1/me`

Body: `{ display_name?, avatar_url? }`.

#### `POST /v1/me/merge-anonymous`

Body: none. Server reads the `anon_id` cookie.

Behavior: merges anonymous behavioral data into user account. Idempotent. Cookie may be rotated post-merge.

#### `DELETE /v1/me`

Deletes account. Requires re-auth confirmation via Clerk.

### Inquiries

#### `POST /v1/artworks/:id/inquiries`

Body: `{ name, email, message, budget_range? }`.

Response: `{ status: 'pending_verification' | 'sent' }`.

Behavior:
- Rate-limited (see Rate limiting table).
- **Signed-in users**: `email` is overridden with the verified Clerk email; inquiry routes immediately to the artist per their `inquiry_preferences`. Response `status: 'sent'`.
- **Anonymous users**: inquiry is stored in `pending` state. A verification email is sent to `email` containing a tokenized confirm link. Only on click does the inquiry get delivered to the artist. Response `status: 'pending_verification'`. Prevents impersonation and reduces spam.
- Logs `inquiry_submitted` event (anonymous) and `inquiry_delivered` event (after verification or directly for signed-in).

#### `GET /v1/inquiries/verify/:token`

Public, no auth. Confirms an anonymous inquiry. Marks delivered, triggers `inquiry.deliver` Inngest job.

### Artist onboarding

**Shipped (T-012 Phase 1):**

#### `POST /v1/onboarding/start`

Body: `{ display_name, location? }`. Mints an `artists` row for the calling user with `status='pending'`, generates a unique slug via the `slugify` + collision-suffix path (`jane-doe`, `jane-doe-2`, …), and flips `users.is_artist=true`. Returns the new `StudioArtist` payload, 201.

Errors: 400 if the caller already has an `artists` row, if `display_name` is empty/blank, or exceeds 100 chars; 401 if unauthed.

#### `POST /v1/onboarding/complete`

Flips the caller's artist `status: pending → active`. Idempotent — calling on an already-active artist returns the unchanged row, 200. 404 when the caller has no artist row at all.

**Deferred (T-012 Phase 2, requires Inngest):**

#### `POST /v1/onboarding/import`

Body: `{ website_url?, instagram_handle? }`.

Response: `{ import_job_id }`.

Behavior: starts Inngest job that scrapes, processes images, pre-fills metadata. Client polls job status.

#### `GET /v1/onboarding/import/:job_id`

Response: `{ status: 'queued'|'running'|'done'|'failed', progress, result? }`.

#### `POST /v1/onboarding/extract-metadata`

Body: `{ artwork_id, freeform_text, image_url }`.

Response: `{ extracted: { title?, medium?, dimensions?, year?, tags?, price_cents? } }`.

Behavior: calls LLM with structured extraction prompt. Stores artifact in `llm_extraction_artifacts`.

#### `POST /v1/onboarding/polish-statement`

Body: `{ original_text }`.

Response: `{ polished_text }`.

### Artist studio (authed, artist role)

#### `GET /v1/studio/artworks`

Response: artist's artworks with status filter.

#### `POST /v1/studio/artworks`

Body: artwork fields + image upload IDs.

Response: created artwork.

#### `PATCH /v1/studio/artworks/:id`

Body: partial update.

#### `DELETE /v1/studio/artworks/:id`

Soft-delete.

#### `POST /v1/studio/artworks/:id/images`

Body: `{ upload_id, is_primary, display_order }`.

#### `DELETE /v1/studio/artworks/:id/images/:image_id`

#### `GET /v1/studio/analytics`

Query: `range` (7d|30d|90d).

Response: aggregated analytics — stat cards, time series, top artworks, referrers.

#### `GET /v1/studio/inquiries`

Response: inquiries received (if artist uses on-platform inbox).

#### `PATCH /v1/studio/settings`

Body: bio, location, website, socials, artist_statement, commissioning_preferences, inquiry_preferences, visibility.

#### `GET /v1/studio/locations` *(T-038 G3)*

Lists every `artist_locations` row owned by the calling artist — geocoded and pre-geocode. The studio UI uses the pre-geocode rows to render "Locating…" placeholders.

#### `POST /v1/studio/locations` *(T-038 G3)*

Body: `{ kind: 'gallery'|'studio', name, address, website_url?, display_order? }`. Returns the new row (lat/lng null) and fires a background Mapbox forward-geocode. Hard cap: 50 rows per artist.

#### `PATCH /v1/studio/locations/:id` *(T-038 G3)*

Partial update. Editing `address` re-fires the geocode and clears the cached lat/lng/city/country/geocoded_at until it lands. `website_url` accepts `null` (cleared) via the explicit-null PATCH semantics — see `decisions.md` 2026-05-28 for the `deserialize_double_option` helper.

#### `DELETE /v1/studio/locations/:id` *(T-038 G3)*

Soft-delete via `deleted_at`. Row is hidden from every read path immediately.

### Public map

#### `GET /v1/search/map` *(T-038 G5)*

Returns map pins (`artist_locations` rows) matching the active filters. One row per location.

Query params:
- `q` — artwork-tsvector match (pin shows if the artist has *any* matching artwork)
- `medium` — same EXISTS shape as `q`
- `location` — case-insensitive substring on `artist_locations.city`
- `bbox` — `"west,south,east,north"` (lng,lat,lng,lat). Mapbox's `bounds.toArray().flat()` ordering. Validator rejects malformed / inverted / out-of-range with RFC 7807 400.
- `artist` *(T-041)* — exact slug match. Pins down the map to a single artist's venues; powers the "See on full map →" CTA on `/artists/[slug]`. Composes with all other filters.

Hard cap: 500 pins per response. Mapbox GL JS clusters client-side, so the cap leaves room for an interactive map without server-side aggregation. See `decisions.md` 2026-05-28 for why this is a separate endpoint from `/v1/search`.

#### `GET /v1/search/map/cities` *(T-042)*

Returns the top-N cities by venue count, with a centroid + tight bbox per city. Powers the horizontal "city pivot" pills above `/search?map=1` — clicking a pill jumps the map to that city's bbox.

Query params:
- `limit` — how many cities to return (default 12, max 100).

Response: `Array<{ city, country, count, center_lat, center_lng, west, south, east, north }>`. Excludes pre-geocode rows + inactive artists. Ordered by `count DESC, city ASC`.

Single GROUP BY query; no rate-limit layer (light static read). When a city has one pin, `west == east` and `south == north` — the client pads the bbox to a sensible viewport (~5km half-extent) before calling `fitBounds`.

### Admin

V1 admin is direct DB access — there is no admin UI. For 20–30 hand-picked artists in v0, a small set of internal CLI scripts (or `psql` invocations) handles approvals and edits. Pre-built portfolio claim flow is deferred — see `99-deferred.md`.

### Events

#### `POST /v1/events`

Body: `{ event_name, properties?, context? }`.

Behavior: validates, writes to events table. Used for client-side event tracking that PostHog doesn't cover natively (some custom events).

---

## Database schema (Postgres + pgvector)

### `artists`

```sql
id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
user_id uuid REFERENCES users(id) NULL,
slug text UNIQUE NOT NULL,  -- collision strategy: on insert, append -2, -3, ... until unique
display_name text NOT NULL,
bio text,
artist_statement text,
location text,  -- free-text display, e.g. "Berlin, Germany". Editable by artist.
city text,  -- structured city, populated by geocoder
country text,  -- ISO 3166-1 alpha-2, populated by geocoder
lat double precision,  -- nullable; populated by geocoder
lng double precision,  -- nullable; populated by geocoder
geocoded_at timestamptz,  -- last successful geocode
website_url text,
socials jsonb DEFAULT '{}',
commissioning_preferences jsonb,  -- { accepts: bool, types: [], price_min, price_max, notes }
inquiry_preferences jsonb NOT NULL,  -- { type: 'email'|'platform'|'external', email?, url? }
status text NOT NULL DEFAULT 'pending',  -- pending, active, paused, rejected
created_at timestamptz NOT NULL DEFAULT now(),
updated_at timestamptz NOT NULL DEFAULT now(),
deleted_at timestamptz

INDEX (slug), INDEX (status), INDEX (user_id),
INDEX (city), INDEX (country),
INDEX (lat, lng) WHERE lat IS NOT NULL AND lng IS NOT NULL
```

**Slug collisions:** on artist creation, generate slug from `display_name`. If taken, append `-2`, then `-3`, etc., until unique. Helper lives in the API; do not rely on UNIQUE constraint races — check-and-insert in a transaction.

**Geocoding:** when `location` is set or changed, the `artist.geocode` Inngest job calls Mapbox forward-geocode, populates `city`, `country`, `lat`, `lng`, `geocoded_at`. Failures leave fields null; the artist is still searchable by name and artwork, just not by geography. Re-runs are idempotent.

### `artist_locations` *(T-038)*

```sql
id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
artist_id uuid NOT NULL REFERENCES artists(id) ON DELETE CASCADE,
kind text NOT NULL CHECK (kind IN ('gallery', 'studio')),
name text NOT NULL,           -- "Foo Gallery", "Open studio", etc.
address text NOT NULL,        -- street-level, as typed by the artist
city text, country text,      -- populated by geocoder
lat double precision, lng double precision,  -- populated by geocoder
website_url text,
display_order int NOT NULL DEFAULT 0,
geocoded_at timestamptz,      -- last attempt (success or fail)
created_at, updated_at, deleted_at timestamptz,

INDEX (artist_id, display_order) WHERE deleted_at IS NULL,
INDEX (lat, lng) WHERE lat IS NOT NULL AND lng IS NOT NULL AND deleted_at IS NULL,
INDEX (city) WHERE deleted_at IS NULL,
INDEX (created_at) WHERE geocoded_at IS NULL AND deleted_at IS NULL  -- geocode worklist
```

One row per place an artist's work can be seen. Distinct from `artists.{city,country,lat,lng}` which is the artist's "based in" — that's city-level and fuzzy; `artist_locations` is street-level and pinnable. Shows / events as time-bound entities are deferred (`99-deferred.md` Phase 2); when they land, rows here migrate to `spaces` + `space_artists` join.

**Trust model:** self-listed; the public surface labels every pin "Listed by the artist." No admin moderation in v1. See `decisions.md` 2026-05-28.

**Geocoding:** `address` is captured raw on insert; an Inngest function `artist_location.geocode` calls Mapbox v6 forward-geocode and writes back lat/lng/city/country/geocoded_at. Until Inngest is wired (current state), the studio CRUD handlers `tokio::spawn` the same logic — same semantics, replaced with the Inngest call when the runtime lands. Public surfaces (artist profile, `/v1/search/map`) filter out rows where lat/lng are still null.

### `artworks`

```sql
id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
artist_id uuid NOT NULL REFERENCES artists(id),
title text,
description text,
year_created integer,
medium text,
dimensions jsonb,  -- { height, width, depth?, unit: 'cm'|'in' }
price_cents bigint,
currency text DEFAULT 'USD',
availability text NOT NULL DEFAULT 'available',  -- available, sold, not_for_sale, inquire
external_url text,
status text NOT NULL DEFAULT 'draft',  -- draft, published, archived
created_at timestamptz NOT NULL DEFAULT now(),
updated_at timestamptz NOT NULL DEFAULT now(),
deleted_at timestamptz,
published_at timestamptz

INDEX (artist_id, status),
INDEX (status, published_at DESC) WHERE deleted_at IS NULL,
FULLTEXT INDEX on (title, description)  -- via tsvector column
```

### `artwork_images`

One row per uploaded original. Variants (thumb / medium / full) are **not** stored as separate rows — they are generated on demand by an image proxy at the CDN edge.

```sql
id uuid PRIMARY KEY,
artwork_id uuid NOT NULL REFERENCES artworks(id) ON DELETE CASCADE,
s3_key text NOT NULL,  -- the original
width integer,
height integer,
is_primary boolean NOT NULL DEFAULT false,
display_order integer NOT NULL DEFAULT 0,
moderation_status text NOT NULL DEFAULT 'pending',  -- pending, approved, rejected
created_at timestamptz DEFAULT now()

INDEX (artwork_id, display_order)
UNIQUE INDEX one_primary_per_artwork ON artwork_images (artwork_id) WHERE is_primary = true
```

**Image variants** are served via CloudFront in front of an image transform layer (imgproxy on Lambda, or AWS Serverless Image Handler). URL pattern: `https://img.example.com/<size>/<s3_key>`. The CDN caches by URL — no DB rows per variant, no resize jobs at upload, deterministic transforms.

**Cascade vs soft-delete**: `artworks` is soft-deleted (`deleted_at`), so this `ON DELETE CASCADE` only fires if an artwork is hard-deleted (which we don't do via the API — only direct DB / admin script). See **Delete model** below for the consistent policy.

### `artwork_embeddings`

```sql
artwork_id uuid NOT NULL REFERENCES artworks(id) ON DELETE CASCADE,
model_name text NOT NULL,
model_version text NOT NULL,
embedding vector(1024) NOT NULL,  -- adjust to model output dim
created_at timestamptz DEFAULT now(),

PRIMARY KEY (artwork_id, model_name, model_version),
HNSW INDEX on embedding WHERE (model_name, model_version) matches current default
```

Separate table keeps re-embedding safe and lets you run two models in parallel for A/B.

### `tags`

```sql
id uuid PRIMARY KEY,
slug text UNIQUE NOT NULL,
label text NOT NULL,
category text  -- medium, style, mood, subject
```

### `artwork_tags`

```sql
artwork_id uuid REFERENCES artworks(id) ON DELETE CASCADE,
tag_id uuid REFERENCES tags(id),
PRIMARY KEY (artwork_id, tag_id)
```

### `import_sources`

```sql
id uuid PRIMARY KEY,
artwork_id uuid REFERENCES artworks(id) ON DELETE CASCADE,
source_url text NOT NULL,
source_type text,  -- 'website', 'instagram', 'manual'
scraped_at timestamptz,
metadata jsonb
```

### `llm_extraction_artifacts`

```sql
id uuid PRIMARY KEY,
artwork_id uuid REFERENCES artworks(id),
input_text text,
input_image_url text,
output_json jsonb,
model text,
prompt_version text,
created_at timestamptz DEFAULT now()
```

### `users`

```sql
id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
clerk_user_id text UNIQUE NOT NULL,
email text UNIQUE NOT NULL,
display_name text,
avatar_url text,
is_artist boolean DEFAULT false,
is_admin boolean DEFAULT false,
created_at timestamptz DEFAULT now(),
updated_at timestamptz DEFAULT now()

INDEX (clerk_user_id)
```

### `user_collections`

```sql
id uuid PRIMARY KEY,
user_id uuid NOT NULL REFERENCES users(id),
name text NOT NULL,
description text,
is_public boolean DEFAULT false,
share_id text UNIQUE,  -- set when is_public becomes true
created_at timestamptz DEFAULT now(),
updated_at timestamptz DEFAULT now(),
deleted_at timestamptz

INDEX (user_id)
```

### `collection_artworks`

```sql
collection_id uuid REFERENCES user_collections(id) ON DELETE CASCADE,
artwork_id uuid REFERENCES artworks(id),
notes text,  -- schema supports, UI v1.1
display_order integer,  -- schema supports, UI v1.1
added_at timestamptz DEFAULT now(),

PRIMARY KEY (collection_id, artwork_id)
```

### `events`

```sql
id uuid PRIMARY KEY,
anonymous_id uuid,
user_id uuid REFERENCES users(id),
event_name text NOT NULL,
event_schema_version integer NOT NULL,
occurred_at timestamptz NOT NULL,
session_id uuid,
properties jsonb,
context jsonb  -- referrer, device, geo

PARTITION BY RANGE (occurred_at),  -- monthly partitions
INDEX (user_id, occurred_at DESC),
INDEX (anonymous_id, occurred_at DESC),
INDEX (event_name, occurred_at DESC)
```

### `user_profiles`

```sql
user_id uuid PRIMARY KEY REFERENCES users(id),
taste_embedding vector(1024),
preferred_mediums jsonb,
price_range_seen jsonb,
color_affinity jsonb,
interaction_count integer DEFAULT 0,
last_active timestamptz,
profile_updated_at timestamptz

HNSW INDEX on taste_embedding
```

### `neighborhoods`

```sql
id uuid PRIMARY KEY,
slug text UNIQUE NOT NULL,
name text NOT NULL,
description text,
cluster_centroid vector(1024),
representative_artwork_ids uuid[],
artwork_count integer,
computed_at timestamptz

INDEX (slug)
```

### `neighborhood_artworks`

```sql
neighborhood_id uuid REFERENCES neighborhoods(id) ON DELETE CASCADE,
artwork_id uuid REFERENCES artworks(id) ON DELETE CASCADE,
distance_to_centroid real,

PRIMARY KEY (neighborhood_id, artwork_id),
INDEX (neighborhood_id, distance_to_centroid)
```

Refreshed by scheduled Inngest job (weekly initially).

### `inquiries`

```sql
id uuid PRIMARY KEY,
artwork_id uuid REFERENCES artworks(id),
artist_id uuid REFERENCES artists(id),
from_user_id uuid REFERENCES users(id),  -- null for anonymous
from_email text NOT NULL,
from_name text NOT NULL,
message text NOT NULL,
budget_range jsonb,
delivery_channel text,  -- 'email', 'platform'
delivered_at timestamptz,
created_at timestamptz DEFAULT now()

INDEX (artist_id, created_at DESC)
```

### `uploads` (for visual search)

```sql
id uuid PRIMARY KEY,
s3_key text NOT NULL,
anonymous_id uuid,
user_id uuid,
embedding vector(1024),  -- populated by image.embed Inngest job at upload time
moderation_status text NOT NULL DEFAULT 'pending',  -- pending, approved, rejected
created_at timestamptz DEFAULT now(),
expires_at timestamptz  -- 24h default
```

### `eval_set`

Hand-curated ground-truth pairs for search/recommendation quality. Run periodically via the `eval.run` Inngest job; results posted to an admin dashboard (CLI in v1).

```sql
id uuid PRIMARY KEY,
query_type text NOT NULL,  -- 'text', 'image', 'image+modifier'
query_text text,
query_image_s3_key text,
modifiers text[],
expected_artwork_ids uuid[] NOT NULL,  -- ranked, ideal order
notes text,
created_at timestamptz DEFAULT now()
```

Target metric: NDCG@10 on the eval set. Track over time; a release that drops NDCG@10 by more than 5% is blocked from rolling to prod until reviewed.

---

## Delete model

Consistent policy:

- **Soft-delete** (`deleted_at` set, row preserved): `artists`, `artworks`, `user_collections`, `users`.
- **Hard-delete** (row removed): `artwork_images`, `collection_artworks`, `artwork_tags`, `artwork_embeddings`, `uploads`, transient rows.
- All public read queries filter `deleted_at IS NULL` on soft-delete tables.
- `ON DELETE CASCADE` foreign keys only fire if a parent row is hard-deleted. The API never hard-deletes parents; admin scripts may, and the cascade is the safety net.
- A separate `purge` Inngest job runs monthly: hard-deletes any soft-deleted row older than 30 days, cascading through dependents.

---

## Shared types (TypeScript, for client)

Generated from Rust structs via `ts-rs` or hand-written Zod schemas as source of truth.

Core types:
- `ArtworkSummary` — id, slug, title, artist_name, artist_slug, primary_image_url, price_cents, currency, availability
- `Artwork` — ArtworkSummary + full fields, all images, tags, dimensions
- `ArtistSummary` — id, slug, display_name, location, representative_images
- `Artist` — ArtistSummary + bio, statement, socials, website, commissioning
- `Collection` — id, name, description, is_public, share_id, cover_image, artwork_count
- `Neighborhood` — id, slug, name, description, representative_images, artwork_count

---

## Search ranking implementation notes

- Hybrid ranking via RRF:
  1. Keyword rank: Postgres full-text on title, description, artist display_name, tag labels.
  2. Semantic rank: k-NN on artwork_embeddings with query embedding.
  3. Fuse: `rrf_score = 1/(60 + keyword_rank) + 1/(60 + semantic_rank)`.
  4. Apply structured filters (medium, price, size).
  5. Sort by rrf_score DESC.
- Visual search blends image embedding + text modifier embedding:
  - Three candidate sets (image alone, text alone, weighted blend).
  - RRF across all three.
  - Curated modifier buttons use precomputed delta vectors added to the image embedding.
- Recommendations: k-NN against `user_profiles.taste_embedding`, exclude already-seen artworks.
- Similar artworks: k-NN against artwork's own embedding, exclude same artist unless requested.

---

## Inngest jobs

- `artist.geocode` — triggered on artist `location` change. Calls Mapbox forward-geocode, populates `city`, `country`, `lat`, `lng`, `geocoded_at`. Idempotent; null on failure.
- `image.moderate` — calls AWS Rekognition `DetectModerationLabels` on a new image (artwork or visual-search upload). Sets `moderation_status`. Blocks publish/search use if rejected.
- `image.embed` — fetches image, calls embedding API (Jina/Voyage), upserts into `artwork_embeddings` or `uploads.embedding`. Runs in parallel with `image.moderate`; results only used if moderation approves.
- `artist.import.run` — scrapes artist website, downloads images, creates artwork drafts. Instagram import deferred.
- `user_profile.refresh` — rebuilds taste embedding and aggregates from recent events. Cold-start: no profile until N (≥10) qualifying interactions; before that, users see the default homepage.
- `inquiry.deliver` — routes verified inquiry to artist per their preferences.
- `purge` — monthly hard-delete of soft-deleted rows older than 30 days.
- `eval.run` — scheduled run of the hand-curated eval set, computes NDCG@10, persists to a results table.

---

## Third-party services

| Service | Purpose | Free / dev tier |
|---|---|---|
| Neon | Postgres + pgvector | Free tier covers v1 dev; ~$20/mo at modest prod use |
| Clerk | Auth (hosted) | Free up to 10k MAU |
| AWS | Lambda, API Gateway, S3, CloudFront, Rekognition | Pay-as-you-go, near-zero at v1 traffic |
| Inngest | Background jobs | 50k runs/month free |
| Upstash | Redis (rate limiting) | 10k commands/day free |
| PostHog | Product analytics | 1M events/month free |
| Axiom or CloudWatch | Logs + metrics | Axiom 500GB/mo free; CloudWatch built-in |
| Jina / Voyage | Multimodal embeddings | Pay-per-call, cheap at v1 |
| Anthropic / OpenAI | LLM for intake + query rewriting | Pay-per-call |
| Mapbox | Geocoding (forward, artist location) | 100k requests/month free |
| Resend | Transactional email | 3k/month free |

See `04-stack-and-infra.md` for deployment topology.

---

## What's deferred from API/data

See `99-deferred.md` for the full backlog. Includes: pre-built portfolio claim flow + scraping pipeline + outreach log + claim tokens; admin submission queue UI; algorithmic neighborhoods (HDBSCAN + LLM labeling); public user profiles; saved searches / alerts; notifications system; multi-currency conversion; artist-to-artist messaging; collaborative collections; marketplace transactions; reviews/ratings.

All schema extensions for these can be added without breaking v1 contracts.

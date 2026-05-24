# Art Discovery Platform — API + Data Spec (v1)

## Principles

- REST-ish over HTTPS. JSON in/out. No GraphQL.
- Zod schemas shared between Rust API contracts (via generated types) and Next.js client.
- Auth via Clerk JWTs in `Authorization: Bearer` header. API validates JWT, extracts `user_id`.
- Anonymous requests carry `X-Anonymous-Id` header (UUID from first-party cookie).
- Every endpoint instrumented — request logged, event emitted on meaningful actions.
- Cursor-based pagination (not offset).
- Versioned under `/v1/` from day one.

## Auth model

- Clerk handles all auth UI and session management.
- API validates Clerk JWT on requests requiring auth.
- `X-Anonymous-Id` header present on every request (including authed). Used for merging behavioral data.
- On sign-in/up, client calls `POST /v1/me/merge-anonymous` with the anonymous_id to trigger server-side profile merge.

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
- `sort` (relevance|newest|price_asc|price_desc).
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

Behavior: validates file, stores in S3 under `uploads/` prefix, generates thumbnail. Embedding happens lazily when used in search. Uploads auto-expire after 24h if unused.

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

Body: `{ anonymous_id }`.

Behavior: merges anonymous behavioral data into user account. Idempotent.

#### `DELETE /v1/me`

Deletes account. Requires re-auth confirmation via Clerk.

### Inquiries

#### `POST /v1/artworks/:id/inquiries`

Body: `{ name, email, message, budget_range? }`.

Response: `{ status: 'sent' }`.

Behavior: looks up artist's inquiry_preferences, routes accordingly. Logs `inquiry_submitted` event.

### Artist onboarding

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

#### `POST /v1/onboarding/publish`

Publishes the artist's portfolio (moves artworks from draft to published, sets artist status active).

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

### Pre-built portfolio claim

#### `GET /v1/claim/:token`

Response: pre-built portfolio data for preview. Public.

#### `POST /v1/claim/:token/claim`

Body: `{ email }`.

Behavior: sends Clerk magic link to email. On verification, associates artist profile with new user account.

#### `POST /v1/claim/:token/takedown`

Body: `{ reason? }`.

Behavior: takes down the pre-built portfolio. No auth required — token is the auth. Logs takedown in outreach_log.

### Admin

#### `GET /v1/admin/submissions`

Query: `status`, `cursor`.

Response: paginated submissions.

#### `POST /v1/admin/submissions/:id/approve`

#### `POST /v1/admin/submissions/:id/reject`

Body: `{ reason? }`.

### Events

#### `POST /v1/events`

Body: `{ event_name, properties?, context? }`.

Behavior: validates, writes to events table. Used for client-side event tracking that PostHog doesn't cover natively (some custom events).

---

## Database schema (Postgres + pgvector)

### `artists`

```sql
id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
user_id uuid REFERENCES users(id) NULL,  -- null for pre-built, set on claim
slug text UNIQUE NOT NULL,
display_name text NOT NULL,
bio text,
artist_statement text,
location text,
website_url text,
socials jsonb DEFAULT '{}',
commissioning_preferences jsonb,  -- { accepts: bool, types: [], price_min, price_max, notes }
inquiry_preferences jsonb NOT NULL,  -- { type: 'email'|'platform'|'external', email?, url? }
status text NOT NULL DEFAULT 'pending',  -- pending, active, paused, rejected
is_prebuilt boolean DEFAULT false,
created_at timestamptz NOT NULL DEFAULT now(),
updated_at timestamptz NOT NULL DEFAULT now(),
deleted_at timestamptz

INDEX (slug), INDEX (status), INDEX (user_id)
```

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

```sql
id uuid PRIMARY KEY,
artwork_id uuid NOT NULL REFERENCES artworks(id) ON DELETE CASCADE,
s3_key text NOT NULL,
variant text NOT NULL,  -- thumb, medium, full, original
width integer,
height integer,
is_primary boolean DEFAULT false,
display_order integer NOT NULL DEFAULT 0,
created_at timestamptz DEFAULT now()

INDEX (artwork_id, display_order)
UNIQUE (artwork_id, variant) WHERE is_primary  -- one primary per variant
```

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

### `artwork_submissions`

```sql
id uuid PRIMARY KEY,
artist_id uuid REFERENCES artists(id),
submitted_by_user_id uuid REFERENCES users(id),
payload jsonb NOT NULL,  -- the submitted data
status text NOT NULL DEFAULT 'pending',
reviewed_by uuid REFERENCES users(id),
review_note text,
created_at timestamptz DEFAULT now(),
reviewed_at timestamptz
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

### `artist_claim_tokens`

```sql
token text PRIMARY KEY,  -- URL-safe random
artist_id uuid NOT NULL REFERENCES artists(id),
created_at timestamptz DEFAULT now(),
expires_at timestamptz,
claimed_at timestamptz,
takedown_at timestamptz,
takedown_reason text
```

### `outreach_log`

```sql
id uuid PRIMARY KEY,
artist_id uuid REFERENCES artists(id),
contacted_at timestamptz,
channel text,  -- 'email', 'instagram_dm', 'manual'
recipient text,
message_template text,
response_status text,  -- 'no_response', 'claimed', 'declined', 'takedown'
response_at timestamptz,
notes text
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
embedding vector(1024),  -- populated lazily
created_at timestamptz DEFAULT now(),
expires_at timestamptz  -- 24h default
```

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

- `artwork.embedding.generate` — triggered on artwork publish. Fetches image, calls embedding API, upserts into artwork_embeddings.
- `artist.import.run` — scrapes artist website/Instagram, downloads images, creates artwork drafts.
- `user_profile.refresh` — rebuilds taste embedding and aggregates from recent events.
- `neighborhoods.recompute` — weekly, runs HDBSCAN clustering over all published artwork embeddings, labels with LLM, updates neighborhoods table.
- `inquiry.deliver` — routes inquiry to artist per their preferences.
- `claim.email` — sends claim magic link.
- `eval.run` — scheduled run of the hand-curated eval set, posts metrics to admin dashboard.

---

## Third-party services

| Service | Purpose | Free tier |
|---|---|---|
| Neon | Postgres + pgvector | Generous, enough for v1 |
| Clerk | Auth | 10k MAU |
| Vercel | Frontend + Next.js API | Hobby tier covers v1 |
| AWS | Lambda, S3, CloudFront, API Gateway | Pay-as-you-go, near-zero at v1 traffic |
| Inngest | Background jobs | 50k runs/month |
| PostHog | Analytics | 1M events/month |
| Axiom | Logs + metrics | 500GB/month |
| Jina / Voyage | Multimodal embeddings | Pay-per-call, cheap at v1 |
| Anthropic / OpenAI | LLM for intake + query rewriting | Pay-per-call |
| Resend | Transactional email | 3k/month |

---

## What's deferred from API/data

- Public user profile pages (no endpoint for `/users/:username`).
- Saved searches / alerts.
- Notifications system.
- Multi-currency conversion.
- Artist-to-artist messaging.
- Collaborative collections.
- Marketplace transaction handling.
- Review / rating endpoints.

All schema extensions for these can be added without breaking v1 contracts.

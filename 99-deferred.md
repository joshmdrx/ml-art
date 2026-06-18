# Art Discovery Platform — Deferred (Post-V1)

Single source of truth for features intentionally left out of v1. Schema decisions in v1 are forward-compatible with everything here — extensions only, no breaking contract changes.

## Cold-outreach / scale-up

### Pre-built portfolio claim flow

The idea: for cold outreach, scrape a target artist's public website, build a private portfolio preview behind a tokenized URL, email them a link with "this is a draft of how your page could look — claim and edit, or ignore."

**Why deferred:**
- V0 onboards 20–30 artists via direct outreach. Doesn't need automation.
- Even private (token-gated, never publicly indexed), republishing scraped imagery has legal nuance — wants a real ToS, artist agreement, and DMCA process before scaling.
- Want to validate that artists *want* to be on the platform before investing in a scraper-driven funnel.

**What v1 schema already supports:**
- Add back `artist_claim_tokens` table (token, artist_id, expires_at, claimed_at, takedown_at).
- Add back `outreach_log` table.
- Re-introduce `artists.is_prebuilt` flag.
- Add `/v1/claim/:token` GET / claim / takedown endpoints.
- All without touching the rest of the API.

**Scope when revived:**
- Website scraping only (not Instagram — TOS-restricted, unreliable).
- Token-gated preview pages, robots.txt disallowed, no public indexing.
- Default-private — claim makes public; ignore = never public.
- Auto-expire unclaimed previews after 90 days; purge images.

### Admin submission queue UI

The `/admin/submissions` page from the original spec.

**Why deferred:** at 20–30 hand-picked artists, direct DB / CLI is faster than building UI. Build when the queue exceeds ~5 pending at a time and you find yourself wanting filters and notes.

## Search & recommendation quality

### Algorithmic neighborhood discovery (HDBSCAN + LLM labeling)

Original spec called for a weekly scheduled job that clusters all published artwork embeddings with HDBSCAN and uses an LLM to label clusters.

**Status update 2026-06-17:** promoted to active roadmap as `T-057` per `decisions.md` 2026-06-17 "Algorithmic neighbourhoods as primary discovery primitive." Remainder of this entry is historical context.

**Why deferred (original v1):** clustering needs a corpus large enough to produce meaningful clusters (~thousands of artworks). At v1 launch (hundreds), it produces noise + one mega-blob. V1 uses **manually curated** neighborhoods: 6–12 hand-picked themes with hand-picked representative artworks.

**Trigger to revive:** when published artwork count crosses ~2000, prototype clustering against the manually curated set and compare. Only ship algorithmic if it's at least as coherent as manual.

### Personalized recommendation surface on homepage

`user_profiles.taste_embedding` exists in v1 schema but isn't surfaced on the homepage in v1 — the homepage is the same for everyone (curated neighborhoods + recent additions).

**Status update 2026-06-17:** promoted to active roadmap as `T-055` (taste vector + refresh) + `T-056` ("For you" row).

**Why deferred (original v1):** cold-start. New users have no profile. Showing empty rec slots reads worse than not having them. The taste embedding builds up via the `user_profile.refresh` scheduled job and becomes useful once a user has ≥10 qualifying interactions.

**When revived:** add a "For you" row on the homepage that only renders if `interaction_count ≥ 10`. Falls back to curated neighborhoods otherwise.

### Query rewriting / expansion

Use the LLM to expand short queries ("moody coastal" → "moody coastal landscape, overcast skies, muted palette, atmospheric"). Could improve recall on text search.

**Why deferred:** adds latency to every search; needs eval-set validation before shipping.

## Monetization paths

All deferred. See kill/pivot metric in `04-stack-and-infra.md`. When the metric is hit or we decide to monetize earlier, candidates in priority order:

1. **Artist subscriptions** — paid tier for advanced analytics (visitor demographics, search-query keywords that brought viewers, conversion to click-out), priority placement in curated neighborhoods, custom domain on artist page.
2. **Lead-gen / inquiry fees** — small per-inquiry fee charged to artists, with a free monthly quota.
3. **Curated marketplace** — opt-in transactions, platform takes a commission. Major build; only if there's clear demand from both sides.
4. **Sponsored neighborhoods** — gallery / institutional sponsorship of themed neighborhoods. Risk: corrupts curation.

Each path has schema implications; revisit before committing.

## Geographic discovery (post-v1 track)

**Update 2026-05-28:** The lean v1 slice of this track — `artist_locations`, Mapbox geocoding, studio CRUD, artist-profile map, `/search?map=1` — shipped as `T-038`. See `decisions.md` 2026-05-28 for the scope-and-tradeoffs entry that promoted it from this doc into v1. What remains below is still post-v1.

V1 originally shipped only the structured `city`, `country`, `lat`, `lng` on artists with `location` and `near_me` filters on `/v1/search`. The T-038 work added per-artist `artist_locations` rows (gallery + studio kinds), live Mapbox geocoding, the artist-profile map widget, and the `/search?map=1` map mode. The full geographic story below is bigger and worth planning as a coherent track rather than piecemeal additions.

### Phase 1 — Geographic neighborhoods *(map view shipped in v1 as T-038)*

- ~~**Map view** on `/search`.~~ ✅ **Shipped in T-038 G5.** Grid/Map toggle, clustered pins, URL-synced bounds.
- **Geographic neighborhoods** — manually curated, same UX as semantic neighborhoods but selected by location. "London painters", "Mexico City ceramicists", "Pacific Northwest". Editorial work; same `neighborhoods` table with an additional `kind text` discriminator (`semantic` | `geographic`). **Still deferred.**
- **Artist "based in" filter** on the homepage and neighborhood pages. **Still deferred.**

Schema additions: minor. `neighborhoods.kind`, that's it.

### Phase 2 — Spaces and events as first-class entities

Naming note: **"spaces"** rather than "galleries". The product is for independent artists, many of whom intentionally avoid the commercial gallery system. "Spaces" includes artist-run project spaces, fairs, pop-ups, residencies, community studios — broader, more native to the indie ecosystem. Galleries fit *within* spaces as one type.

**New entities:**

```sql
spaces (
  id uuid PRIMARY KEY,
  slug text UNIQUE NOT NULL,
  name text NOT NULL,
  kind text NOT NULL,  -- 'gallery', 'project_space', 'artist_run', 'fair', 'residency', 'pop_up'
  description text,
  city text, country text, lat double precision, lng double precision,
  address text,  -- street-level, optional
  website_url text,
  socials jsonb,
  status text NOT NULL DEFAULT 'pending',
  created_at, updated_at, deleted_at
)

events (
  id uuid PRIMARY KEY,
  slug text UNIQUE NOT NULL,
  space_id uuid REFERENCES spaces(id),
  title text NOT NULL,
  description text,
  kind text,  -- 'opening', 'show', 'fair', 'open_studio', 'pop_up'
  starts_at timestamptz NOT NULL,
  ends_at timestamptz NOT NULL,
  status text NOT NULL DEFAULT 'pending',
  created_at, updated_at, deleted_at
)

event_artists (
  event_id uuid REFERENCES events(id),
  artist_id uuid REFERENCES artists(id),
  PRIMARY KEY (event_id, artist_id)
)

event_artworks (
  event_id uuid REFERENCES events(id),
  artwork_id uuid REFERENCES artworks(id),
  PRIMARY KEY (event_id, artwork_id)
)
```

**New surfaces:**

- `/spaces` index + filters (city, kind)
- `/spaces/[slug]` — space profile, upcoming events, represented artists
- `/events` index — calendar view ("this weekend", "next month")
- `/events/[slug]` — event detail
- Map view enhanced to overlay spaces + active events
- Studio gets a "I'm in this event" claim flow
- Admin moderation queue for new spaces (more important than artist moderation because spaces are venues for trust)

**Moderation considerations:** spaces are higher-stakes than artists — they're physical venues people will visit. Need stronger admin review. Real-address verification for fairs / large venues. Probably not v2; v2.5 or later.

### Phase 3 — Discovery loops between geography and content

- "Artists showing near you this month" — combine `near_me` + active events
- "Upcoming shows of artists you've saved" — events filtered by saved-artists set
- Email digest opt-in: weekly summary of events in user's city

### What to revisit before building any of this

- Is geography a top-3 differentiator for the product, or a nice-to-have? Phase 2 is a meaningful build (~weeks of work). Don't start without that conviction.
- Where does monetization sit for spaces? Free listings vs paid promotion?
- Trust model for events — anyone listing? Verified artists only? Verified spaces only?

## Product features

- **Public user profile pages** (`/users/:username`) — schema supports a future `users.username`, `users.public_collections` flag.
- ~~**Saved searches & alerts** — table `saved_searches`, scheduled job to email matches.~~ Promoted 2026-06-17 → `T-059`.
- **Notifications system** — in-app inbox for inquiry replies, saved-artwork-now-available, etc.
- **Multi-currency conversion** — artworks already store currency; need an exchange-rate service and per-user display preference.
- **Artist-to-artist messaging.**
- **Collaborative collections** — multi-user collections, perms model.
- **Reviews / ratings** — high abuse surface area; skip until needed.
- **Editorial / weekly email** — curator picks a neighborhood and 12 artworks; could be high-engagement.
- **Dark mode** — token system in place via Tailwind, just defer the second palette.
- **Drag-to-reorder in collections** — schema supports (`collection_artworks.display_order`), UI deferred.
- **Notes per saved artwork in a collection** — schema supports (`collection_artworks.notes`), UI deferred.
- **PWA / mobile app** — responsive web for v1.

## Infrastructure & tooling

- **Axiom for logs** — CloudWatch covers v1. Migrate when query latency or cost becomes painful.
- **Real-time updates** (WebSockets / SSE) for inquiry inbox — polling is fine at v1 scale.
- **Multi-region** — single-region deploy for v1.
- **Read replicas** on Postgres — Neon's branching covers dev; production read replicas not needed at v1 traffic.
- **CI/CD blue-green or canary** — Terraform apply is the deploy in v1; canary later.

## Process / governance

- **Real legal review of ToS, Privacy, DMCA, artist agreement** — v1 launches with templated docs (Termly or fork). Real lawyer review when revenue is on the table or at first inbound legal contact.
- **SOC2 / GDPR data processing addendum infrastructure** — when first enterprise / institutional customer asks.
- **Multi-admin role granularity** — v1 has a single boolean `users.is_admin`. Add role table when we have more than two admins.

---

When reviving any item from this doc: move the relevant section into the appropriate primary spec doc, then delete it here. This file should shrink over time, not grow as a graveyard.

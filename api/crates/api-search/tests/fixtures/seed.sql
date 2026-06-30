-- Test fixture for api-search integration tests.
--
-- Loaded by `#[sqlx::test(fixtures("seed"))]`. Migrations run before this,
-- so all extensions and tables exist. Known UUIDs let tests assert specific
-- relationships.

-- ─────────────────────────────────────────────────────────────────────────────
-- Users (1; used for collections / inquiries in later tests)
-- ─────────────────────────────────────────────────────────────────────────────

INSERT INTO users (id, clerk_user_id, email, display_name, is_artist, is_admin) VALUES
  ('99999999-9999-9999-9999-999999999999', 'user_test_99', 'test@example.com', 'Test User', false, false),
  -- alice + bob: used by the collections tests to assert ownership boundaries.
  -- Alice is also an artist (linked below); bob is intentionally NOT — the
  -- /v1/studio/* tests rely on bob hitting the "no artist for this user" 404.
  ('88888888-8888-8888-8888-888888888888', 'user_test_alice', 'alice@example.com', 'Alice', true,  false),
  ('77777777-7777-7777-7777-777777777777', 'user_test_bob',   'bob@example.com',   'Bob',   false, false),
  -- T-083 — dedicated admin identity for tests. NOT an artist so the
  -- /v1/admin/* surface can be exercised without colliding with the
  -- studio-side tests that use bob as the "non-artist" baseline.
  ('66666666-6666-6666-6666-666666666666', 'user_test_admin', 'admin@example.com', 'Admin', false, true);

-- ─────────────────────────────────────────────────────────────────────────────
-- Artists (3)
--   alice: London/GB, active
--   bruno: Berlin/DE, active
--   carmen: no location, active
-- ─────────────────────────────────────────────────────────────────────────────

INSERT INTO artists (
    id, user_id, slug, display_name, bio, location, city, country, lat, lng, geocoded_at,
    inquiry_preferences, status
) VALUES
  -- alice-test linked to user_test_alice so studio endpoints resolve.
  ('aaa11111-1111-1111-1111-111111111111',
   '88888888-8888-8888-8888-888888888888',
   'alice-test', 'Alice Test',
   'Painter based in London.', 'London, GB', 'London', 'GB', 51.5074, -0.1278, now(),
   '{"type":"platform"}'::jsonb, 'active'),
  -- bruno-test + carmen-test have no Clerk user (demo-style). Tests use them
  -- to assert that an artist's user can't reach into another artist's data.
  ('aaa22222-2222-2222-2222-222222222222', NULL, 'bruno-test', 'Bruno Test',
   'Sculptor based in Berlin.', 'Berlin, DE', 'Berlin', 'DE', 52.5200, 13.4050, now(),
   '{"type":"platform"}'::jsonb, 'active'),
  ('aaa33333-3333-3333-3333-333333333333', NULL, 'carmen-test', 'Carmen Test',
   'Printmaker; location private.', NULL, NULL, NULL, NULL, NULL, NULL,
   '{"type":"platform"}'::jsonb, 'active'),
  -- T-083 — pending artist for the admin queue tests. No user link;
  -- the queue lists by `ar.status = 'pending'` regardless of ownership.
  ('aaa44444-4444-4444-4444-444444444444', NULL, 'dora-pending', 'Dora Pending',
   'Painter awaiting admin approval.', 'Manchester, GB', 'Manchester', 'GB', 53.4808, -2.2426, now(),
   '{"type":"platform"}'::jsonb, 'pending');

-- ─────────────────────────────────────────────────────────────────────────────
-- Artworks (6 — 5 published, 1 draft)
-- ─────────────────────────────────────────────────────────────────────────────

INSERT INTO artworks (
    id, artist_id, title, description, medium, price_cents, currency,
    availability, status, is_demo, published_at
) VALUES
  -- Alice
  ('bbb11111-1111-1111-1111-111111111111',
   'aaa11111-1111-1111-1111-111111111111',
   'Blue Morning', 'A quiet study in cobalt.', 'Painting',
   100000, 'USD', 'available', 'published', false, now() - interval '5 days'),
  ('bbb22222-2222-2222-2222-222222222222',
   'aaa11111-1111-1111-1111-111111111111',
   'Crimson Field', 'Warm-palette composition.', 'Painting',
   250000, 'USD', 'available', 'published', false, now() - interval '4 days'),
  -- Bruno
  ('bbb33333-3333-3333-3333-333333333333',
   'aaa22222-2222-2222-2222-222222222222',
   'Stone Form I', 'Carved limestone.', 'Sculpture',
   NULL, 'EUR', 'inquire', 'published', false, now() - interval '3 days'),
  ('bbb44444-4444-4444-4444-444444444444',
   'aaa22222-2222-2222-2222-222222222222',
   'Stone Form II', NULL, 'Sculpture',
   500000, 'EUR', 'sold', 'published', false, now() - interval '2 days'),
  -- Carmen
  ('bbb55555-5555-5555-5555-555555555555',
   'aaa33333-3333-3333-3333-333333333333',
   'Linocut Study', 'Black and white print.', 'Print',
   80000, 'USD', 'available', 'published', false, now() - interval '1 day'),
  -- Draft — must NOT appear in any public query
  ('bbb66666-6666-6666-6666-666666666666',
   'aaa33333-3333-3333-3333-333333333333',
   'Hidden Sketch', NULL, 'Print',
   NULL, 'USD', 'available', 'draft', false, NULL);

-- T-080 — backfill `price_gbp_cents` from the seeded fx_rates so the
-- price-filter tests don't see NULL on every fixture row. The migration's
-- own backfill runs BEFORE the fixture INSERTs, so we have to replay it
-- here against the just-inserted rows.
UPDATE artworks a
SET price_gbp_cents = ROUND(a.price_cents * fx.rate_to_gbp)::bigint
FROM fx_rates fx
WHERE fx.code = a.currency AND a.price_cents IS NOT NULL;

-- ─────────────────────────────────────────────────────────────────────────────
-- Artwork images (1 per published artwork; the draft has none)
-- ─────────────────────────────────────────────────────────────────────────────

INSERT INTO artwork_images (
    id, artwork_id, s3_key, width, height, is_primary, display_order,
    moderation_status, moderation_reason
) VALUES
  (gen_random_uuid(), 'bbb11111-1111-1111-1111-111111111111', 'test/alice/1.jpg', 1200,  900, true, 0, 'approved', NULL),
  (gen_random_uuid(), 'bbb22222-2222-2222-2222-222222222222', 'test/alice/2.jpg', 1200,  900, true, 0, 'approved', NULL),
  (gen_random_uuid(), 'bbb33333-3333-3333-3333-333333333333', 'test/bruno/1.jpg',  900, 1200, true, 0, 'approved', NULL),
  (gen_random_uuid(), 'bbb44444-4444-4444-4444-444444444444', 'test/bruno/2.jpg',  900, 1200, true, 0, 'approved', NULL),
  (gen_random_uuid(), 'bbb55555-5555-5555-5555-555555555555', 'test/carmen/1.jpg', 1000, 1000, true, 0, 'approved', NULL),
  -- T-083.3 — rejected image for admin override tests. Pinned id so
  -- the tests can target it without joining. is_primary=false so it
  -- doesn't collide with the existing primary on bbb55555.
  ('eee11111-1111-1111-1111-111111111111',
   'bbb55555-5555-5555-5555-555555555555',
   'test/carmen/rejected.jpg', 800, 800, false, 1, 'rejected', 'EXPLICIT_NUDITY');

-- ─────────────────────────────────────────────────────────────────────────────
-- Embeddings (1024-dim, 1.0 at distinct positions so similarities are well-defined)
-- ─────────────────────────────────────────────────────────────────────────────

-- model_version='v2' matches the unified label set by migration 0009
-- (T-024 fold-in). See `decisions.md` 2026-05-27.
INSERT INTO artwork_embeddings (artwork_id, model_name, model_version, embedding)
SELECT
    artwork_id,
    'jinaai/jina-clip-v2',
    'v2',
    (
        SELECT array_agg(CASE WHEN j = pos THEN 1.0::real ELSE 0::real END ORDER BY j)
        FROM generate_series(0, 1023) j
    )::vector(1024)
FROM (VALUES
    ('bbb11111-1111-1111-1111-111111111111'::uuid, 0),
    ('bbb22222-2222-2222-2222-222222222222'::uuid, 1),
    ('bbb33333-3333-3333-3333-333333333333'::uuid, 2),
    ('bbb44444-4444-4444-4444-444444444444'::uuid, 3),
    ('bbb55555-5555-5555-5555-555555555555'::uuid, 4)
) AS v(artwork_id, pos);

-- ─────────────────────────────────────────────────────────────────────────────
-- Neighborhood (1)
-- ─────────────────────────────────────────────────────────────────────────────

INSERT INTO neighborhoods (
    id, slug, name, description, kind,
    representative_artwork_ids, artwork_count, is_featured, display_order
) VALUES
  ('ccc11111-1111-1111-1111-111111111111', 'test-vibes', 'Test Vibes',
   'A test neighborhood spanning Alice and Bruno.', 'curated',
   ARRAY['bbb11111-1111-1111-1111-111111111111'::uuid,
         'bbb22222-2222-2222-2222-222222222222'::uuid,
         'bbb33333-3333-3333-3333-333333333333'::uuid],
   3, true, 0);

INSERT INTO neighborhood_artworks (neighborhood_id, artwork_id) VALUES
  ('ccc11111-1111-1111-1111-111111111111', 'bbb11111-1111-1111-1111-111111111111'),
  ('ccc11111-1111-1111-1111-111111111111', 'bbb22222-2222-2222-2222-222222222222'),
  ('ccc11111-1111-1111-1111-111111111111', 'bbb33333-3333-3333-3333-333333333333');

-- ─────────────────────────────────────────────────────────────────────────────
-- Artist locations (T-038)
--   alice: 2 rows — one geocoded (gallery, public), one pre-geocode (studio, hidden)
--   bruno: 1 row — geocoded gallery in Berlin
--   carmen: 0 — used as the "no locations" fallback case
-- ─────────────────────────────────────────────────────────────────────────────

INSERT INTO artist_locations (
    id, artist_id, kind, name, address, city, country, lat, lng,
    website_url, display_order, geocoded_at
) VALUES
  -- Alice — public gallery pin
  ('ddd11111-1111-1111-1111-111111111111',
   'aaa11111-1111-1111-1111-111111111111',
   'gallery', 'Test Gallery London', '1 Test St, London EC1A 1AA',
   'London', 'GB', 51.5155, -0.0922,
   'https://test-gallery.example', 0, now()),
  -- Alice — pre-geocode studio (lat/lng NULL); must be hidden from public surfaces
  ('ddd22222-2222-2222-2222-222222222222',
   'aaa11111-1111-1111-1111-111111111111',
   'studio', 'Studio (by appointment)', '99 Test Lane, London',
   NULL, NULL, NULL, NULL,
   NULL, 1, NULL),
  -- Bruno — public gallery pin
  ('ddd33333-3333-3333-3333-333333333333',
   'aaa22222-2222-2222-2222-222222222222',
   'gallery', 'Berlin Project Space', '12 Teststraße, 10115 Berlin',
   'Berlin', 'DE', 52.5300, 13.3850,
   'https://berlin-space.example', 0, now());

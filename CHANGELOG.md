# Changelog

Engineering-facing log of what shipped, in date order. Strategic / architectural
rationale lives in `decisions.md`.

## 2026-05-28 — Visual search by upload (T-010 Phase B)

Threading the upload through to `/v1/search` so an uploaded image
becomes the semantic anchor. Pure vector when no `q`; hybrid composition
when both are set ("things like this image AND about painting").

- **`GET /v1/search?image_upload_id=<uuid>`**
  - Looks up `uploads.embedding` and uses it as the semantic anchor.
    Unknown id → 404 (UUID is unguessable enough to act as a
    capability; abuse-driven hardening lands later); row exists but
    embedding NULL → 400 (upload is mid-flight, retry)
  - Image anchor wins over text-derived anchor for the *semantic* side
    when both are set. `q` still drives the *keyword* CTE, so
    `q=zzz-no-match&image_upload_id=…` is "rank by image vector,
    keyword side returns nothing" — which is exactly what we want for
    "things like this image"
  - Pure-vector search (only `image_upload_id`, no `q`) falls through
    the existing hybrid path with an empty keyword CTE — no new code
    branch needed
  - All structured filters (medium / price / availability / location /
    near) still apply

- **Tests (5 new in `uploads_test.rs`)**
  - `search_by_image_upload_returns_vector_ranked_results` — seed an
    upload at pos=0 (Blue Morning's fixture vector); assert it ranks
    first
  - `search_by_unknown_image_upload_id_404s`
  - `search_by_upload_without_embedding_400s` — covers the mid-flight
    race where the row exists but the embed step didn't complete
  - `search_image_upload_wins_over_text_for_semantic_anchor` —
    explicit precedence assertion
  - `search_by_image_upload_respects_filters` — image anchor +
    `medium=Sculpture` excludes the painting at pos=0

- **Tangential test-isolation fix**
  - The two `core::images` tests both mutated `IMAGE_BASE_URL` and
    occasionally raced when Cargo ran them in parallel. Merged into a
    single test that sequences the env-mutating assertions. Caught it
    on the first cross-suite run after adding Phase B; would have hit
    CI eventually

- **Verified**
  - `cargo clippy --workspace --all-targets -- -D warnings` ✅
  - `cargo fmt --check` ✅
  - Rust **119** (was 115) — +5 search-by-upload tests, −1 merged
    images test
  - Vitest 20, Playwright 25 unchanged, all green

## 2026-05-27 — Image upload endpoint (T-010 Phase A)

Visual-search entry point. `POST /v1/uploads/image` accepts a multipart
image, validates it, PUTs to the `uploads/` bucket (MinIO in dev, S3 in
prod), writes a `uploads` row, and embeds inline via the T-036 pipeline
so the vector is ready before the first search using it.

- **Handler `api-search::uploads`**
  - Multipart parse via `axum::extract::Multipart` (new `multipart`
    feature on the axum workspace dep)
  - 10MB body cap; content-type allowlist for jpeg/png/webp; rejects
    empty bodies + missing `image` field
  - Identity: signed-in user → `user_id`, otherwise `anonymous_id` from
    `X-Anonymous-Id`. Both routes accepted
  - Filename extension is honored when reasonable (jpg/jpeg/png/webp);
    otherwise derived from content-type — defensive against
    `foo.exe` masquerading as image/png
  - S3 PUT happens *before* DB insert so a failed PUT leaves no orphan
    row. Inline embed runs after insert; failure leaves a NULL-embedding
    row that the future `expires_at`-driven cleanup job evicts

- **New `core::object_store::ObjectStore`**
  - Wraps `aws-sdk-s3` behind a small `put` / `public_url` / `is_real`
    surface. `Inner::Real` for prod & MinIO; `Inner::Memory` for tests
  - `ObjectStore::new(...)` configures the SDK from `Config` knobs
    (endpoint override, region, static creds). `force_path_style(true)`
    when an endpoint URL is set so MinIO works without virtual-hosted
    addressing
  - `ObjectStore::for_tests(bucket)` is an in-memory store —
    integration tests run without MinIO. Mirrors the
    `Embedder::with_fixed_vector` pattern (explicit test variant, not
    env-gated)
  - `test_get(key)` lets test assertions peek at what was stored

- **`Config` additions**
  - `uploads_bucket`, `s3_endpoint_url`, `s3_region`, `s3_access_key`,
    `s3_secret_key`, `uploads_public_url_prefix`
  - Defaults wire dev MinIO (`http://localhost:9000`, creds `dev` /
    `devpassword`, public URL `http://localhost:9000/uploads`)

- **Rate limiting**: `/v1/uploads/image` reuses the `inquiry_limit`
  policy at 3/hr. Per `03-api-data-spec.md` the target is 20/hr; a
  separate `uploads_limit` policy + knob lands when we have signal
  that the inquiry-policy share is the wrong shape

- **Dev-environment note**
  - `http://localhost:9000` isn't reachable from Jina's workers, so
    live dev uploads succeed through the PUT + DB-insert steps and
    then 502 at the embed call. Documented inline in `uploads.rs`.
    Real end-to-end requires either staging (CloudFront-fronted S3)
    or a tunnel for local MinIO (ngrok / cloudflared) with
    `UPLOADS_PUBLIC_URL_PREFIX` overridden

- **Tests**
  - 6 new in `uploads_test.rs`: signed-in writes row + embeds,
    anonymous uses anon_id, extension derived from content-type,
    rejects non-image content-type, rejects empty body, rejects
    missing `image` field
  - 5 existing test helpers updated to thread `ObjectStore::for_tests`
    through `AppState`

- **`.env` + `.env.example`** gain the S3/MinIO vars; dev `.env`
  prefilled with the MinIO creds from `docker-compose.dev.yml`

- **Verified**
  - `cargo clippy --workspace --all-targets -- -D warnings` ✅
  - `cargo fmt --check` ✅
  - Rust **115** (was 109) — +6 uploads tests
  - Vitest 20, Playwright 25 unchanged, all green
  - Live `POST /v1/uploads/image` against running dev API confirmed
    multipart + S3 PUT + DB insert work; embed step fails as expected
    given the localhost-from-Jina constraint above

- **Build cost**
  - `aws-sdk-s3` + `aws-config` add ~30 transitive deps and ~30s to a
    cold release build. Worth it for the working-with-S3 ergonomics;
    revisit if Lambda cold-start becomes a real number

## 2026-05-27 — Studio portfolio page (T-011 Phase 3)

Third slice. `/studio` is now a real page — artists can see their full
portfolio, filter by status, create / edit / delete artworks, and
manage images via a modal. No LLM (`T-012`) and no bulk upload (`T-010`)
yet.

- **`/studio` page (server-rendered)**
  - Signed-out → /sign-in?redirect_url=/studio
  - Signed-in non-artist → empty state (same shape as `/studio/settings`)
  - Signed-in artist → header w/ status, grid of artworks, status
    filter pills, "+ New artwork" button. Settings link top-right
  - Status filter is URL-driven (`?status=draft|published|archived`)
    so refreshes / back-navigation land on the same view

- **`StudioPortfolio` client component**
  - Grid of `ArtworkCard`s — thumbnail or "no image" placeholder,
    title, medium, status badge (Draft / Published / Archived). Edit
    button on each card opens the modal in edit mode
  - "All / Drafts / Published" filter pills (toolbar role, aria-pressed)
  - Empty state for `all` shows "Your portfolio is empty" + a primary
    "+ New artwork" CTA; for `draft`/`published` views, a hint to
    switch back to All

- **`ArtworkEditModal` — single component for create + edit**
  - `target` prop: `"new"` = create form, `<uuid>` = edit mode (loads
    detail lazily via the new `loadArtworkForEdit` server action)
  - Full field set: title, medium, description, year created, price,
    currency, availability, external URL, status. Status select is
    disabled during create (forced 'draft') and active after save
  - Image manager appears below the form once the artwork exists:
    grid of current images with "Primary" badge + per-image Remove,
    plus an "Add" input that accepts an s3_key. Adding an image
    triggers the inline `process_image` (T-036) embedding pipeline
    on the API side. The s3_key affordance is explicit-but-temporary
    — `T-010` will mint validated keys server-side
  - Delete button on edit; confirm() prompt; closes the modal on success
  - Create flow stays in the modal after save so the user can add an
    image to the freshly-created row without a page transition

- **New server actions** (`app/actions/studio.ts`)
  - `loadArtworkForEdit(id)` — wraps `getStudioArtwork` for client use.
    Required because `lib/api.ts` transitively imports
    `@clerk/nextjs/server` (server-only) via `apiFetch`; the Phase 3
    build broke when the modal imported the lib function directly
  - `createArtwork`, `patchArtwork`, `deleteArtwork`,
    `addArtworkImage`, `removeArtworkImage` — all revalidate
    `/studio` + the public `/artworks/:id` so changes reflect
    immediately

- **`lib/api.ts` additions**
  - `StudioArtworkSummary`, `StudioArtworkDetail`, `StudioImage`,
    `CreateArtworkBody`, `PatchArtworkBody` types
  - `listMyArtworks`, `getStudioArtwork`, `createStudioArtwork`,
    `patchStudioArtwork`, `deleteStudioArtwork`,
    `addStudioArtworkImage`, `removeStudioArtworkImage` clients

- **Tests**
  - 1 new Playwright spec — `18-studio-portfolio-signed-in.spec.ts` —
    empty-state smoke for the non-artist test user. Happy-path
    create/edit/delete is exercised at the Rust integration tier
    against alice-test (28 studio tests, unchanged this phase)
  - Vitest 20 unchanged
  - Playwright **25** (was 24)

- **Verified**
  - `cargo clippy --workspace --all-targets -- -D warnings` ✅
  - `cargo fmt --check` ✅
  - `pnpm exec tsc --noEmit` ✅ / `pnpm exec eslint .` ✅
  - Rust 109 unchanged (no API changes this phase)
  - Live `/studio` page reachable; new-user flow exercised manually
    against alice-test via Bearer-driven studio API calls

- **Lessons folded back**
  - Client components must call server actions, not `lib/api.ts`
    functions directly — caught the hard way when the first Playwright
    run died with `'server-only' cannot be imported from a Client
    Component module`. Added a CONTRIBUTING.md note in the next sweep

## 2026-05-27 — Studio settings + public-surface visibility honesty (T-011 Phase 2)

The second slice of the studio. Settings API + page, and — load-bearing
— a fix to make `artists.status='paused'` actually hide an artist's
work from public surfaces. The "Unpublish portfolio" toggle is a lie
without that fix.

- **API: `PATCH /v1/studio/settings`**
  - Editable: `bio` (max 4k), `artist_statement` (max 8k), `location`
    (max 200), `website_url` (max 500, must be http(s)),
    `socials`/`commissioning_preferences`/`inquiry_preferences` (jsonb),
    `status` (self-serve toggle: `active` ↔ `paused` only)
  - Changing `location` clears the geocoded shadow fields
    (`city`/`country`/`lat`/`lng`/`geocoded_at`) so the future async
    geocode job re-runs against the new value
  - 404 for non-artist users (same ownership pattern as the rest of
    `/v1/studio/*`); 400 with detail for length/URL/status violations

- **Public-surface status filter (`artists.status = 'active'`)**
  - Until now, only `inquiries.rs` filtered on artist status — search,
    artwork detail, artwork similar, neighborhoods, and collections
    detail all ignored it. Paused artists' work would have stayed
    visible everywhere, making the Unpublish toggle silently broken
  - Added `AND ar.status = 'active'` to all five public-facing query
    sites (search RRF + nearest-sort branches, artwork detail, artwork
    similar, neighborhoods detail, collections detail)
  - Already-correct surfaces (artist profile, inquiry create) untouched

- **Web: `/studio/settings` page**
  - Server-renders `getStudioMe()`; redirects signed-out users to
    `/sign-in?redirect_url=…`; renders "you're not set up as an artist"
    empty state for signed-in non-artists
  - New `StudioSettingsForm` client component — single-column form
    (bio, statement, location, website) + a separate visibility toggle
    section that persists independently. Diffs against `initial` so the
    Save button only enables when something changed and only dirty
    fields land in the PATCH body
  - New server action `updateStudioSettings(body)` revalidates
    `/studio/settings`, `/studio`, and the artist's public page so
    Unpublish → public page 404 happens immediately
  - New `getStudioMe()` + `updateStudioSettings()` in `lib/api.ts`
  - New `StudioArtist` + `StudioSettingsPatch` types

- **Tests**
  - 7 new Rust integration tests in `studio_test.rs`:
    `patch_updates_bio_and_statement`,
    `patch_changing_location_clears_geocoded`,
    `patch_rejects_bad_status` (covers `pending`/`rejected`/empty),
    `patch_rejects_non_http_url`,
    `patch_404s_for_non_artist`,
    `paused_artist_disappears_from_search`,
    `paused_artist_artwork_detail_404s`
  - 2 new Playwright specs in `17-studio-settings-signed-in.spec.ts` —
    empty-state for non-artist user + page-renders smoke. Happy-path
    edits stay covered at the Rust integration tier (linking a
    Clerk-test user to an artist row in Playwright is a future
    test-fixture concern)

- **Verified**
  - `cargo clippy --workspace --all-targets -- -D warnings` ✅
  - `cargo fmt --check` ✅
  - Rust **109** tests (was 102) — +7 studio
  - Vitest 20 unchanged, Playwright **24** (was 22)
  - Full suite + live `/studio/settings` page reachable

## 2026-05-27 — Studio API: artwork CRUD + ownership (T-011 Phase 1)

First slice of the artist studio. API only — pages land in Phases 2-4.
The structural piece is: `users.is_artist` + `artists.user_id` were
schema-defined but unused; this turns them into the load-bearing
ownership boundary every `/v1/studio/*` endpoint enforces.

- **New endpoints**
  - `GET    /v1/studio/me` — returns the artist linked to the caller
    (404 for collectors / non-artist users — same shape leak-prevention
    pattern as the rest of `/v1/me/*`)
  - `GET    /v1/studio/artworks?status=draft|published|archived|all` —
    list this artist's portfolio (incl. drafts)
  - `POST   /v1/studio/artworks` — create (defaults to `status='draft'`)
  - `GET    /v1/studio/artworks/:id` — full detail w/ images
  - `PATCH  /v1/studio/artworks/:id` — partial update; status
    `draft → published` transition stamps `published_at` in SQL
  - `DELETE /v1/studio/artworks/:id` — soft-delete via `deleted_at`
  - `POST   /v1/studio/artworks/:id/images` — add image by `s3_key`.
    First image lands as primary by default. Primary image add inline-
    calls `artwork_embeddings::process_image` (T-036) so vector search
    finds the work immediately. Rekognition gating + async-job lift
    are `T-008` + a future ticket
  - `DELETE /v1/studio/artworks/:id/images/:image_id` — remove image

- **Ownership pattern**
  - New `studio::current_artist_id(pool, user)` helper — the single
    place where `AuthedUser → artists.user_id → artist_id` resolves.
    Returns `NotFound` if the user has no linked artist. Every studio
    handler calls this first
  - All SQL joins through `artworks.artist_id = $artist_id` so
    cross-artist access (Alice editing Bruno's artwork) returns 404,
    same shape as the collections `me/*` pattern

- **New types** (`studio::artworks`)
  - `StudioArtworkSummary` — list-response row, includes `status` and
    `primary_image_url` (drafts can be imageless)
  - `StudioArtworkDetail` — `summary` + `description / year_created /
    dimensions / external_url / images[]`. The detail view powering
    the future edit modal
  - `StudioImage` — `id / s3_key / url / width / height / is_primary /
    display_order / moderation_status`. Public images endpoints don't
    surface `moderation_status` but studio does, so artists see why
    something's hidden

- **Database**
  - Migration `0010_studio_user_artist_link.sql` — partial unique index
    `artists (user_id) WHERE user_id IS NOT NULL`. NULL `user_id` (the
    WikiArt seed) explicitly allowed; live users are 1:1 with artists
  - Test fixture `seed.sql` — alice user gets `is_artist=true` +
    `artist.user_id` link to `aaa11111…`. Bob stays a non-artist for
    the 404 tests. Dev DB also updated via direct `psql -f`

- **Tests** — 21 new integration tests in `studio_test.rs`
  - `/v1/studio/me`: returns-linked-artist, 404-for-non-artist,
    401-without-auth
  - GET list: only-my-artworks, includes-drafts, status-filter,
    404-for-non-artist
  - POST create: defaults-to-draft, rejects-bad-availability,
    404-for-non-artist
  - PATCH: updates-title, status-published-stamps-published_at,
    rejects-bad-status, alice-cannot-patch-brunos
  - DELETE: soft-deletes (idempotent), 2nd delete is 404
  - Images: first-is-primary-and-embeds (via T-036),
    rejects-second-primary, remove, remove-cross-artist-404
  - Detail: returns-images, 404-for-cross-artist

- **New test helper** `app_with_auth_and_fixed_vector(pool, vec)` —
  the third `app_*` variant; combines `JwtVerifier::for_tests()` with
  `Embedder::with_fixed_vector`. Studio image-add tests need both

- **Verified**
  - `cargo clippy --workspace --all-targets -- -D warnings` ✅
  - `cargo fmt --check` ✅
  - Rust **102** tests (was 81) — +21 studio integration
  - Vitest 20, Playwright 22 unchanged, full Playwright sweep still green
  - Live `/v1/studio/me` returns 401 unauthed; live `/v1/health` ok;
    migration 0010 applied to dev DB

## 2026-05-27 — Embedding pipeline for new artworks (T-036, T-024)

Closes the structural gap the audit identified: until now nothing in the
production code path took a freshly-created `artworks` row → called Jina
→ wrote an `artwork_embeddings` row. Seeded artworks had embeddings
(Python local pass at seed time) but anything new would be invisible to
vector search. Studio create (`T-011`) and upload-driven visual search
(`T-010`) both depend on this piece existing.

- **`Embedder::embed_image_from_url(url)`** — calls Jina's image
  endpoint with the `{image: "<url>"}` payload shape (URL works because
  our images sit at MinIO `localhost:9000` in dev, CDN in prod —
  fetchable by Jina's workers in both cases). Errors out when
  `JINA_API_KEY` is unset (image embedding is required, not best-effort
  like text). Test path: `with_fixed_vector` returns the canned vector
  for the image branch too, parallel to `embed_text`.

- **`core::artwork_embeddings` (new module)**
  - `write(pool, artwork_id, model_name, model_version, vector)` — INSERT
    ... ON CONFLICT upsert keyed on the composite PK. Re-embedding the
    same artwork with the same model is idempotent; writing a *different*
    `model_version` (future `'v3'`) adds a row alongside for safe A/B
  - `process_image(pool, embedder, artwork_id, image_url)` — composes
    embed + write. The single function studio create handlers will call.
    The future `image.process` Inngest job + Rekognition moderation gate
    sit either side of this function as scope grows

- **`Embedder::model_name()` + `model_version()` accessors** —
  `process_image` reads them off the embedder so callers don't have to
  hold their own copy of the strings

- **T-024 model_version unification folded in**
  - New migration `db/migrations/0009_normalize_model_version.sql` —
    one-shot UPDATE: `('local'|'api') → 'v2'` on both
    `artwork_embeddings` (2000 rows in dev) and `query_embedding_cache`
    (9 rows). Idempotent: re-running matches no rows
  - Python `LocalJinaClipEmbedder` + `JinaClipEmbedder` both return
    `'v2'` for `model_version` (were `'local'` / `'api'`)
  - Rust `Config` default flipped to `'v2'`; same for
    `Embedder::disabled` + `Config::for_tests`
  - `api/.env` + `api/.env.example` updated to `EMBEDDING_MODEL_VERSION=v2`
  - Test fixture `seed.sql` writes `'v2'`
  - T-024 removed from TODO.md (folded here)

- **Tests**
  - 5 new integration tests in `tests/artwork_embeddings_test.rs`:
    - `write_round_trips_through_pgvector` — exact f32 byte equality
    - `write_is_idempotent_under_same_pk` — second write doesn't dup
    - `write_with_different_version_creates_second_row` — A/B semantics
    - `process_image_writes_a_row_with_v2_label` — end-to-end via the
      fixed-vector embedder
    - `process_image_makes_artwork_findable_via_similar` — creates a
      fresh artwork, processes, hits `/v1/artworks/:id/similar` through
      the full Axum stack, asserts the new row is in the result set
  - New helper `embedder_with_fixed_vector(pool, vec)` in
    `tests/common/mod.rs` for pipeline tests that don't want the router
  - Tallies: Rust **81** (was 76). Vitest 20, Playwright 22 unchanged.

- **`process_image` is sync-call today, not a job queue**
  - Studio handlers (when `T-011` lands) call this inline at first
  - When moderation lands (`T-008` Rekognition), it'll have to become
    async since Rekognition is itself async — at that point we add a
    `JobQueue` trait + an Inngest impl + a Tokio-spawn impl, and the
    `process_image` contract stays the same
  - Decision logged inline in the module doc

## 2026-05-27 — Code review pass: standards + refactors before T-011

Audit, fixes, and convention-setting before the studio milestone doubles
the handler count. Five new entries in `decisions.md` underpin this pass;
see them for full rationale.

- **Lint baseline restored**
  - `cargo fmt --check` clean (formatted `artwork.rs` + a few others)
  - `cargo clippy --workspace --all-targets -- -D warnings` clean:
    - `auth.rs:225` — `cloned_ref_to_slice_refs` → `std::slice::from_ref`
    - `models.rs:45` — `derivable_impls` → `#[derive(Default)] + #[default]`
    - test files — per-file `#![allow(dead_code)]` for Deserialize-only
      contract docs; `common/mod.rs` module-wide allow for cross-test helpers
    - `rate_limit_test.rs:40` — `needless_borrows_for_generic_args`
  - `pnpm exec eslint` clean:
    - `SaveModal.tsx` + `InquiryModal.tsx` — moved close-reset off of
      `useEffect`-on-`!open` into the dialog's `onOpenChange` handler.
      Per-line `eslint-disable react-hooks/set-state-in-effect` on the
      two intentional state-machine transitions, with comments
    - `search/page.tsx:186` — `<a href="/">` → `<Link href="/">`
    - **New project-wide ESLint rule**: `no-console` (allows
      `warn` / `info`). Bare `console.error` is rejected; new code uses
      `reportError(err, ctx)` from `lib/reportError.ts`

- **`CONTRIBUTING.md`** — codifies the conventions we've evolved by
  accident: docs hierarchy, Rust style (module docs, error type,
  `for_tests` constructor, row-type location, dynamic SQL pattern),
  TS/React style (`apiFetch`, server actions, `reportError`, client-
  component opt-in), SQL/migration conventions, the new TODO comment
  format, test-naming, commit/PR rules. References `decisions.md`
  throughout.

- **`lefthook.yml`** — pre-commit hooks run the same checks CI does,
  scoped to the staged files (`cargo fmt`, `cargo clippy`, `eslint`,
  `tsc --noEmit`) plus a regex that rejects bare `TODO:` / `FIXME` in
  added lines, enforcing the new `TODO(T-NNN):` format. Install with
  `lefthook install` (one-time). Decision logged at `decisions.md`
  2026-05-27 — Pre-commit hooks.

- **`core::images::url_for_s3_key`** — extracts the duplicated
  `image_url()` helper from `artwork.rs`, `neighborhoods.rs`, and
  `me/collections.rs` into a single function with its own unit tests.
  All three call sites now import from `ml_art_core::images`.

- **`AuthedUser` axum extractor** — `api-search/src/extractors.rs`
  implements `FromRequestParts<Arc<AppState>>` for a newtype wrapper
  around `core::auth::User`. The newtype is local so the orphan rules
  let us own the impl. Replaces 9 inline
  `auth::authenticate(&headers, &state.jwt_verifier, &state.pool)`
  calls with `AuthedUser(user): AuthedUser` in handler signatures.
  Inquiry create handler uses `Option<AuthedUser>` for the
  signed-in-or-anonymous branch.

- **`web/src/lib/reportError.ts`** — one-function shim wrapping
  `console.error` with a `[err]` prefix + structured context.
  Migrated 9 call sites from `console.error("…failed", e)` →
  `reportError(e, { surface, id })`. Future Sentry/Axiom integration is
  a one-file change.

- **Stale TODOs cleaned**
  - `inquiries.rs:7,16` — replaced `(TODO T-XXX)` placeholder X's with
    real `T-032` references in the module doc
  - `inquiries.rs:191` — `TODO:` → `TODO(T-032):`
  - `search.rs:107` — `T-018` mis-cite → `TODO(T-037)` (new ticket)
  - `config.rs:90` — `TODO.md T-024` → `TODO(T-024)`

- **TODO.md additions**
  - `T-036` Embedding pipeline for new artworks (the structural gap
    the audit identified — blocks studio)
  - `T-037` Cursor pagination on `/v1/search` and friends

- **Doc refresh**
  - `README.md` — "no auth / no save / no inquiry" claim was 6 weeks
    out of date; rewrote the "what works today" table to reflect
    Stage-1-through-3 reality, including rate limiting + the
    deferred-email caveat
  - `01-page-spec.md`, `02-component-library.md`, `03-api-data-spec.md`
    — each gets an "Aspirational" header line pointing at `CHANGELOG`
    + `decisions.md` as the truth for shipped behavior. Matches the
    decision logged this morning
  - `README.md` docs index now lists `CONTRIBUTING.md` first and tags
    the specs as aspirational

- **Verified**
  - `cargo fmt --all --check` ✅
  - `cargo clippy --workspace --all-targets -- -D warnings` ✅
  - Rust **76** tests (was 74) — 65 integration + 11 core unit (added
    2 for `core::images::url_for_s3_key`)
  - Vitest 20 unchanged
  - Playwright 22 unchanged
  - Full E2E suite green against the freshly-built binary

## 2026-05-27 — FilterBar component (T-023)

One pill row shared by `/search` and `/neighborhoods/[slug]`, URL-driven so
every filter combination is bookmarkable.

- **API: `GET /v1/neighborhoods/:slug` accepts filter params**
  - Same shape `/v1/search` already accepts: `medium`, `price_min`,
    `price_max`, `availability`. `location` is intentionally absent — the
    slug already pins place
  - Built with the same `AssertSqlSafe(format!(...))` + `PgArguments` dynamic
    pattern `search.rs` uses; empty filters add no SQL noise
  - Empty-string params (`?medium=&availability=`) are no-ops, matching
    `/v1/search` leniency so the UI's "All" affordance doesn't need
    special-casing

- **`web/src/components/FilterBar.tsx`**
  - Client component; reads URL via `useSearchParams`, writes via
    `router.push(basePath + "?" + applyFilterParam(...))`
  - Pills use `@radix-ui/react-dropdown-menu` (already installed); each pill
    has `aria-pressed` so check-state is observable to a11y tools and to
    Playwright without class selectors
  - `availableFilters: FilterKind[]` prop lets each surface render its
    subset — `/search` gets all four pills, `/neighborhoods/[slug]` omits
    `location`
  - "Clear filters" link appears only when any owned filter is active and
    nukes them all in one router push (leaves unrelated params like `q`
    alone)
  - "Location" is a free-text pill (open-ended values); the others are
    enumerated dropdowns

- **`web/src/lib/filterBar.ts`** (extracted for testability)
  - `applyFilterParam(current, update)` — pure URL builder with null/""
    removal semantics
  - `PRICE_BUCKETS` — four curated bucket tokens (`u500`, `500-2k`,
    `2k-10k`, `10kplus`) that round-trip through `priceParamsFromToken` /
    `bucketTokenFromPriceParams`
  - `MEDIUM_OPTIONS` — 12 art-historical styles, sized to match the WikiArt
    seed corpus. Replace with a server-fed aggregation when real artists
    supply custom mediums (T-017 facet counts is the same query)
  - `AVAILABILITY_OPTIONS` — matches the `artworks.availability` CHECK
    constraint enum directly

- **Page wiring**
  - `/search` page parses `medium / price / availability / location` from
    `searchParams` and threads them into the API call; `price` bucket token
    → `price_min` / `price_max` happens via `priceParamsFromToken` at page
    render time so the API never sees the bucket token
  - `/neighborhoods/[slug]` does the same minus location; `getNeighborhood`
    now takes an optional `filters: NeighborhoodFilters` arg

- **Tests**
  - Rust integration: 5 new tests in `neighborhoods_test.rs` —
    `filters_by_medium`, `filters_by_availability`, `filters_by_price_range`,
    `filters_combine`, `empty_filters_no_op`
  - Vitest: 9 new tests in `filterBar.test.ts` covering `applyFilterParam`
    update / remove semantics and the price-bucket round-trip
  - Playwright: 3 new tests in `16-filter-bar.spec.ts` — choose-a-medium-
    updates-URL-and-results, clear-filters, neighborhood-pill-with-no-
    location-pill assertion
  - Tallies: Rust **74** (was 69) — 65 integration + 9 core unit. Vitest
    **20** (was 11). Playwright **22** (was 19).

## 2026-05-27 — Save-modal membership awareness (T-029)

Save modal now renders accurate check-state on open — clicking a "checked"
row actually unsaves, and the indicator reflects reality after a navigation
or full page reload. Closes the asymmetry where adds were idempotent but
the UI couldn't tell which collections held the artwork.

- **API: `GET /v1/me/collections?artwork_id=<uuid>`**
  - Optional query param; when present, each `CollectionSummary` carries
    `contains_artwork: bool` reflecting whether that artwork is currently
    in the collection
  - SQL uses an `EXISTS` correlated subquery against `collection_artworks`;
    no per-row roundtrip, no N+1
  - Omitting the param leaves `contains_artwork` at `false` on every row —
    backward compatible with the existing `/collections` index page caller
  - Malformed UUID → 400 via axum's `Query` extractor

- **Model**
  - `CollectionSummary.contains_artwork: bool` added with `#[serde(default)]`
    so existing fixtures and the create/patch/detail handlers (which don't
    select the column) keep deserializing cleanly
  - `CollectionRow` uses `#[sqlx(default)]` to absorb the same — no duplicate
    row struct, no behavior change for non-list handlers

- **Web**
  - `lib/api.ts::listMyCollections` accepts `{ artworkId, init }` instead of
    bare `init`; sole caller (the index page) keeps working because the new
    shape is fully optional
  - `actions/collections.ts::fetchMyCollectionsForArtwork` passes through
    the id and seeds the `saved` Set from the per-row flag — replaces the
    old `new Set<string>()` stub and its inline "to do" comment
  - `SaveModal` toggle button gains `aria-pressed={isSaved}` so the
    check-state is observable to a11y tools and to Playwright without
    brittle class selectors

- **Tests**
  - Rust integration: 2 new tests in `collections_test.rs` —
    `collections_list_contains_artwork_flag` (truth table across yes/no/unrelated
    artwork ids) and `collections_list_rejects_malformed_artwork_id`
  - Playwright: 1 new spec `15-save-membership-signed-in.spec.ts` — create
    a collection inline, close the modal, re-open, assert the row is
    rendered with `aria-pressed="true"` (the only path that exercises the
    server-side flag end-to-end)
  - Tallies: Rust **69** (was 67) — 60 integration + 9 core unit. Playwright
    **19** (was 18). Vitest 11 unchanged.

## 2026-05-27 — Rate limiting (T-007)

In-process per-key rate limiting on the two surfaces that exist today, sized
to cap paid embedding spend before edge protection lands.

- **New: `ml_art_core::middleware::rate_limit`**
  - `RateLimiters` — `governor` (GCRA / leaky-bucket) keyed limiter, one bucket per `(policy, key)`
  - `extract_key(headers)` — precedence: Bearer token → `X-Anonymous-Id` → `X-Forwarded-For` first hop → `"fallback"`
  - `search_limit` + `inquiry_limit` axum middleware functions, attached at the `MethodRouter` level so route ownership is explicit (no `route_layer` reordering footguns)

- **Limits (defaults, override via env)**
  - `/v1/search` — 60/min (`RATE_LIMIT_SEARCH_PER_MIN`)
  - `POST /v1/artworks/:id/inquiries` — 3/hr (`RATE_LIMIT_INQUIRY_PER_HOUR`)
  - `/v1/uploads/image` 20/hr + `/v1/events` 200/min defined as constants; layers wire up when those routes land
  - Master kill switch: `RATE_LIMIT_DISABLED=true` (defaults to `true` in `Config::for_tests` so the existing 53 integration tests run unchanged)

- **429 response shape**
  - `ApiError::RateLimited { retry_after_secs: u64 }` — the variant now carries the value instead of being a unit
  - `IntoResponse` sets `Retry-After: <secs>` alongside the existing `Content-Type: application/problem+json`
  - Body: standard RFC 7807 `{ type, title, status: 429, detail }`

- **Tests**
  - 9 new unit tests in `core::middleware::rate_limit` — key extraction precedence, per-key isolation, per-policy isolation, burst denial, disabled bypass
  - 5 new integration tests in `tests/rate_limit_test.rs` — burst → 429 with Retry-After, per-key isolation (anon A vs anon B), per-policy isolation (search bucket ≠ inquiry bucket), bypass-when-disabled smoke
  - New helper `app_with_rate_limit(pool, search_per_min, inquiry_per_hour)` in `tests/common/mod.rs` for tests that need the limiter enabled at a low quota

- **Edge layers logged for the deploy milestone**
  - `T-034` AWS WAF rate-based rule in front of Lambda — protects against volumetric attacks before any cold start
  - `T-035` Vercel edge middleware on `/search` + write server actions — per-IP burst guard at the public frontdoor
  - Both gated on real deploy infra (no `infra/` directory yet); see `decisions.md` 2026-05-27

- **Test tally**
  - Rust: **67** (was 53) — 53 existing integration + 9 new core unit + 5 new integration
  - Playwright: 18 unchanged; Vitest: 11 unchanged

## 2026-05-27 — Close T-030 (write-flow Playwright coverage complete)

Bookkeeping turn. T-030 stayed open after the 2026-05-26 doc-refresh because its
acceptance list deferred signed-in flows to T-031. T-031 landed the same day, so
T-030's full acceptance is now satisfied by:

- `09-inquire-anonymous.spec.ts` — modal → "Check your inbox" → dev verify link → "Sent."
- `10-save-signed-out.spec.ts` — redirect to `/sign-in?redirect_url=…` preserved
- `11-inquiry-verify-bogus.spec.ts` — "Link doesn't look right" for unknown token
- Signed-in branch covered by 12 / 13 / 14 (T-031)

Also cleaned a stale comment block in `10-save-signed-out.spec.ts` that still
pointed at the "Gap: signed-in flows" TESTING.md section that no longer exists
and at the now-landed `T-031`. T-030 removed from TODO.md.

Verified: `make test-e2e` → 18 passed (20.3s).

## 2026-05-26 — Signed-in Playwright coverage via Clerk testing helper

T-031 closed using Clerk's official testing path instead of a custom web bypass.

- **`@clerk/testing` integration**
  - `e2e/tests/auth.setup.ts` runs in a dedicated `setup` Playwright project: signs up `e2e-<stamp>+clerk_test@example.com` (Clerk auto-accepts OTP `424242` per their test-email convention), waits for `/me` to return success (confirms the chain reaches our DB), and writes browser state to `e2e/.auth/user.json`
  - Per-test `setupClerkTestingToken({ page })` bypasses Smart CAPTCHA so headless Chromium can drive the sign-up + token-refresh paths
  - `playwright.config.ts` adds two projects: `chromium` (anonymous tests, `*signed-in*` excluded) and `chromium-authed` (storageState consumer, only `*signed-in*` specs)
  - `e2e/.env.local` (gitignored) holds the Clerk dev keys for the testing helper

- **Three new signed-in specs**
  - `12-save-signed-in.spec.ts` — open Save modal on artwork detail, create a fresh collection inline, see it appear
  - `13-inquire-signed-in.spec.ts` — email pre-filled and read-only, submit, immediate "Sent" (no email-verify step)
  - `14-collections-signed-in.spec.ts` — `/collections` renders without redirect for an authenticated user

- **Test tally**:
  - Playwright: **18 specs** (was 14): 14 anonymous + 1 setup + 3 signed-in, ~17s end-to-end including Clerk sign-up
  - Rust integration / Vitest unchanged: 53 / 11

- **Docs updated**
  - `TESTING.md` — replaces the "open gap on signed-in flows" with the live coverage description; refreshed counts and timing
  - `decisions.md` — new entry "Clerk testing helper for E2E (real auth, not a web bypass)" explaining the choice over a custom bypass
  - `TODO.md` — `T-031` struck through (landed via @clerk/testing); no replacement needed

## 2026-05-26 — Documentation refresh + Playwright write-flow coverage

- **Docs sync** — caught up to actual state:
  - `TODO.md` cleaned end-to-end: removed 9 completed items (T-005, T-006, T-009, T-013, T-021, T-025–T-028) that had drifted; added `T-030` (Playwright write-flow coverage), `T-031` (web test-mode session bypass), `T-032` (real Resend delivery), `T-033` (anon→user merge endpoint), `T-029` (save-modal membership awareness)
  - `TESTING.md` reflects: 53 Rust integration tests, the test-mode JwtVerifier pattern, explicit gap on signed-in Playwright coverage, refreshed stub-strategy table (Clerk Rust ✓, Clerk web pending, Resend documented)
  - `decisions.md` adds two entries: test-mode JwtVerifier (explicit-constructor pattern + why not env-flag) and cross-user resource access returning 404-not-403 (existence-leak rationale)

- **Playwright write-flow coverage** (3 new specs, 14 total):
  - `09-inquire-anonymous.spec.ts` — full end-to-end: search → artwork → Inquire modal → fill form → "Check your inbox" state → click dev verify link → "Sent."
  - `10-save-signed-out.spec.ts` — clicking Save while signed-out redirects to `/sign-in?redirect_url=…` with the artwork URL preserved
  - `11-inquiry-verify-bogus.spec.ts` — verify page with unknown token renders "Link doesn't look right"

- **Test counts**:
  - Rust integration: 53 (unchanged this turn)
  - Playwright: **14** (was 11)
  - Vitest: 11 (unchanged)
  - Full suite still under 12s locally

## 2026-05-26 — Inquiry flow (anonymous email-verify + signed-in direct)

Closes the second user-write loop: send a message to an artist about a specific artwork.

- **Rust API** (`api-search/src/inquiries.rs`)
  - `POST /v1/artworks/:id/inquiries` — branches on auth:
    - **Signed-in:** body `email` ignored; uses Clerk-verified email from `users` row. Inquiry created `verified_at = delivered_at = now()`. Response `{ id, status: "delivered" }`.
    - **Anonymous:** body `email` required; 32-char URL-safe token minted; row created in pending state. Response `{ id, status: "pending_verification" }` (with `debug_verification_token` in dev so manual testing works without Resend).
  - `GET /v1/inquiries/verify/:token` — public; flips `verified_at` + `delivered_at` to now via `COALESCE(... , now())`, so visiting twice is idempotent. 404 for unknown tokens.
  - Delivery channel taken from artist's `inquiry_preferences.type`. Actual email send is **not yet wired** — `T-007` Inngest jobs land alongside Resend integration. The DB row carries enough info for delivery to fire later.
  - Validation: empty/long name (max 120) and message (max 4000) → 400; structural email check on the anonymous path; signed-in path skips the email check entirely.

- **Tests** — 9 new integration tests:
  - signed-in → delivered immediately, body email ignored
  - anonymous → pending; debug token returned; verify endpoint flips both timestamps
  - 404 on missing/draft artworks
  - 400 on empty message, missing email, bad email format
  - verify with unknown token → 404
  - Total now **53 Rust integration tests**, still ~5s suite.

- **Web**
  - `<InquiryModal>` — Radix Dialog with name/email/message/budget. Email pre-fills and goes read-only when signed-in (matches API behavior). Post-submit state branches between "Sent" and "Check your inbox" with a dev-only verify link.
  - `<InquireButton>` replaces the disabled placeholder on `/artworks/[id]`. Anyone can click (no sign-in gate — anonymous can inquire).
  - `/inquiries/verify/[token]` server-rendered confirmation page; idempotent (revisiting is safe).
  - `lib/api.ts`: `submitInquiry`, `verifyInquiry`, `InquiryAck` type.
  - `app/actions/inquiries.ts`: thin `sendInquiry` server action so the Bearer token stays server-side.

- **End-to-end verified**
  - Anonymous flow: POST inquiry → DB row pending → hit verify URL → both timestamps populated → web page shows "Sent."
  - Validation: bad email returns 400; bad URL returns 404 page

## 2026-05-26 — Save to collection (first user-write vertical slice)

Closes the loop: authenticated users can now save artworks into named collections, view them, organize them. First real write surface in the app.

- **Rust API** (`api-search/src/me/collections.rs`)
  - `GET /v1/me/collections` — user's collections with cover-thumb mosaics (batched lookup, no N+1)
  - `POST /v1/me/collections` — create with name (+ optional description / is_public)
  - `GET /v1/me/collections/:id` — single collection + first 60 artworks
  - `PATCH /v1/me/collections/:id` — rename / toggle public (mints/clears share_id)
  - `DELETE /v1/me/collections/:id` — soft-delete
  - `POST /v1/me/collections/:id/artworks` — add (idempotent on (collection, artwork))
  - `DELETE /v1/me/collections/:id/artworks/:artwork_id` — remove
  - All seven enforce ownership via `WHERE user_id = $auth_user_id`; 404 (not 403) for cross-user reads/writes so we don't leak existence
  - Restructured `me.rs` → `me/{mod.rs,current_user.rs,collections.rs}` for room to grow

- **Test infrastructure**
  - `JwtVerifier::for_tests()` — bypasses JWKS, accepts `Bearer test-<sub>` tokens that resolve via the seeded `users` table. Explicit constructor, not env-gated; prod can't accidentally enable it.
  - Fixtures gain `alice` + `bob` users (`user_test_alice`, `user_test_bob`)
  - `app_with_test_auth`, `get_json_authed`, `send_authed` helpers in `tests/common`
  - **12 new collections tests**, including 4 explicit ownership-boundary tests (Bob 404s on every operation against Alice's collections). Full suite: **44 Rust integration tests**, ~5s.

- **Web**
  - `lib/api.ts` — typed client functions for all 6 endpoints
  - `app/actions/collections.ts` — server actions wrapping the client (Bearer token never crosses to the browser)
  - `<SaveModal>` — Radix Dialog with collection list, optimistic toggle, inline "+ New collection" form, error rollback
  - `<SaveButton>` on `/artworks/[id]` — opens modal if signed in, redirects to `/sign-in?redirect_url=…` otherwise
  - `/collections` — index with asymmetric 4-thumb mosaic cards; signed-out → 307 to sign-in
  - `/collections/[id]` — detail view with the saved artworks grid
  - TopNav gains a "Collections" link for signed-in users

## 2026-05-26 — Clerk auth wired end-to-end (read path)

Step 2 of the auth track. User signs up on the web, Rust verifies the JWT, lazy-creates a row in our `users` table.

- **Web (Next.js)**
  - `@clerk/nextjs` installed; `ClerkProvider` in `layout.tsx` with palette-matched appearance overrides
  - `clerkMiddleware()` composes with our existing anon_id middleware in one wrapper
  - `/sign-in/[[...rest]]` and `/sign-up/[[...rest]]` catch-all pages render Clerk's hosted components
  - `TopNav` uses `<Show when="signed-out">` / `<Show when="signed-in">` (Clerk 7.x API) with `<UserButton>` for the avatar
  - `apiFetch` now forwards `Authorization: Bearer <token>` from `auth().getToken()` on every server-side API call, alongside the existing `X-Anonymous-Id`
  - New `/me` debug page that hits `/v1/me` and renders the response — useful while wiring; remove or repurpose later

- **Rust API (`core::auth`)**
  - `JwtVerifier` with JWKS fetch + in-memory cache; refetches if a kid isn't in the cache (Clerk rotates keys)
  - RS256 verification with issuer check + expiry; aud not enforced (Clerk doesn't always set it)
  - `ClerkClaims` extracts `sub` (clerk user id) and timestamps
  - `User { id, clerk_user_id, email, is_admin }` returned by `authenticate(headers, verifier, pool)`
  - First sight of a `clerk_user_id` triggers `JwtVerifier::fetch_clerk_email` against Clerk's `/v1/users/{id}` backend API, then upserts `users` (one extra HTTP request per user, lifetime — no per-request cost)
  - Switched from a `FromRequestParts` extractor with `HasAuthContext` trait to a plain `authenticate()` helper called from handlers. The orphan rules around foreign-trait impls for `Arc<AppState>` weren't worth fighting at this stage.

- **`GET /v1/me`** — returns `{id, clerk_user_id, email, is_admin}` for the authenticated user, 401 otherwise

- **`/v1/health`** now reports `auth_enabled` so the homepage debug surface can tell at a glance

- **Tests** — 32 Rust integration tests still green; `tests/common/mod.rs` updated to build `AppState` with the new `jwt_verifier` field (no-op verifier in tests). The user-extractor isn't covered by tests directly; the negative auth path (401s) is exercised via the live binary.

## 2026-05-26 — Anonymous-id signed cookie + Rust extractor

Step 1 of the auth track (Step 2 = Clerk, waiting on user setup).

- **Next.js middleware** (`web/src/middleware.ts`)
  - Runs at the edge; sets a signed `anon_id` cookie on first request if absent
  - `web/src/lib/anonId.ts`: UUID v7 generator (no BigInt — ES2017 compatible), HMAC-SHA256 sign/verify via Web Crypto, constant-time comparison
  - Cookie: HTTP-only, SameSite=Lax, 1-year expiry, path `/`
  - Path matcher excludes `_next/static/*`, `_next/image`, favicon, and common image extensions so static asset requests don't generate set-cookie noise

- **Server-side fetch helper** (`web/src/lib/api.ts::apiFetch`)
  - Reads cookie via `next/headers::cookies()`, verifies signature, forwards the unsigned UUID to the Rust API as `X-Anonymous-Id`
  - Silent fallback when called outside a request context (e.g. build prerender)
  - All existing API client functions (`searchArtworks`, `getArtist`, `getArtwork`, `getSimilarArtworks`, `listNeighborhoods`, `getNeighborhood`) routed through it

- **Rust extractors** (`api/crates/core/src/auth.rs`)
  - `AnonId(Uuid)` — required; 400 on missing/malformed
  - `OptionalAnonId(Option<Uuid>)` — allowed missing; 400 on malformed
  - Both use the `Pin<Box<dyn Future>>` desugar to match axum-core 0.4.x's `async_trait`-style signature
  - Wired into `search::handle` as `OptionalAnonId` — tracing field populated; no behavior change yet (rate limiting is the next consumer)

- **Tests** (4 new integration tests, total now 32)
  - Search accepts no header / valid header
  - Search 400s on malformed UUID + empty header

- **Decision recorded** — `decisions.md` 2026-05-26 — "anonymous identity: cookie at Next, header to API". Document the boundary-trust model (Next owns signing, API trusts header because production routing prevents direct browser access).

## 2026-05-26 — Makefile + scripts for local-dev orchestration

Replaces the "remember six commands across four directories" workflow.

- **`Makefile`** at repo root with grouped targets:
  - lifecycle — `setup` / `up` / `migrate` / `seed` / `seed-reset` / `down` / `nuke`
  - runners — `dev` (both services), `api`, `web`, `status`
  - tests — `test` (api+web+ml), `test-api`, `test-web`, `test-e2e`, `test-ml`, `test-all`
  - hygiene — `check`, `fmt`
  - util — `psql`, `logs`, `logs-api`, `logs-web`
- **`scripts/migrate.sh`** — applies every `db/migrations/*.sql` via `docker exec psql`; waits for Postgres readiness
- **`scripts/dev.sh`** — runs api + web together; clean Ctrl-C teardown via process-group `pkill`; readiness probes with crash-fallback log dump; logs to `/tmp/{api,web}.log`
- **`scripts/status.sh`** — read-only health check across postgres, minio, mailhog, api, web

## 2026-05-26 — Test infrastructure (Tiers 1–4)

Strategy logged in `TESTING.md`; decision recorded in `decisions.md`.

- **Tier 1 — Rust API integration tests** (28 tests, ~3s locally)
  - `#[sqlx::test]` with per-test ephemeral Postgres; migrations applied automatically from `db/migrations`
  - SQL fixture `tests/fixtures/seed.sql` with known UUIDs (3 artists, 6 artworks, 5 images, 5 embeddings, 1 neighborhood, 1 draft to verify exclusion)
  - `tests/common/mod.rs` helpers: `app_keyword_only`, `app_with_fixed_vector`, `get_json`, `get_status`
  - Refactor: `api-search` is now `[lib] + [bin]` so `build_app` is reachable from `tests/`
  - New embedder constructors: `Embedder::disabled(pool)` and `Embedder::with_fixed_vector(...)` — production paths unchanged
  - `Config::for_tests(database_url)` — deterministic test config
  - Coverage: health, search (no-query / keyword / filters / geographic), artist detail, artwork detail + similar (incl. include_same_artist), neighborhoods index + detail, draft exclusion, all 404s

- **Tier 2 — Playwright E2E** (11 tests, ~4s)
  - New `e2e/` package at repo root
  - 8 spec files covering: home, search keyword, artwork detail, artist portfolio, neighborhoods, 404s, empty state (via impossible filter — vector search makes nonsense-text empty-state hard), location filter
  - Runs against the real local stack; retries once on failure

- **Tier 3 — Vitest** (11 tests, ~180ms)
  - `web/src/__tests__/format.test.ts` — `formatPrice`, `formatDimensions` edge cases
  - Added `typecheck`, `test`, `test:watch` scripts

- **Tier 4 — CI workflows** (`.github/workflows/`)
  - `api.yml` — fmt + clippy + check + tests with Postgres service container
  - `web.yml` — typecheck + lint + Vitest, pnpm cached
  - `e2e.yml` — Postgres + MinIO services, applies migrations + E2E fixture, builds and runs both API and Next, runs Playwright, uploads report on failure
  - `ml.yml` — pytest + ruff for the Python package
  - All four use `paths:` filters so trivial PRs don't trigger everything

## 2026-05-26 — Neighborhoods + homepage section

- **Rust API**
  - `GET /v1/neighborhoods` — lists curated neighborhoods (6 seeded). Batched single-query lookup of representative image URLs from `artwork_images` (no N+1).
  - `GET /v1/neighborhoods/:slug` — header + first 24 artworks, ordered by `distance_to_centroid` ASC NULLS LAST.
  - Models: `Neighborhood` (id, slug, name, description, kind, rep imgs, count, is_featured), `NeighborhoodDetail`.

- **Next.js**
  - `<NeighborhoodCard>` with the asymmetric 3-thumb layout per `01-page-spec.md`
  - `/neighborhoods` index page
  - `/neighborhoods/[slug]` detail page (header + representative strip + artworks grid)
  - Homepage gets an "Explore neighborhoods" section above "Recently added" + "See all →"

- **End-to-end verified**
  - All 6 themed neighborhoods render on homepage and index
  - Detail page (`/neighborhoods/fields-of-color`) shows header, description, artwork grid
  - 404 for unknown slugs propagates via `notFound()`

## 2026-05-26 — Vector search live + polish

- **Vector search wired end-to-end**
  - `dotenvy` loads `api/.env` on startup; `JINA_API_KEY` now flows into the embedder
  - Hybrid ranking (keyword + CLIP-style vector via Jina) returns semantically-relevant results for color/mood/subject queries that previously hit nothing
  - Boundary fix: Jina HTTP API takes the bare model name (`jina-clip-v2`); we keep the full HF id (`jinaai/jina-clip-v2`) in our DB so cached embeddings from Python tooling are usable. The HTTP client strips the `jinaai/` prefix at request time.
  - `query_embedding_cache` populating correctly — each unique query hits Jina exactly once

- **UX polish**
  - Homepage now has one search bar (hero only). `TopNav` gets a `hideSearch` prop; the nav search is gone on `/` because the hero is canonical there.
  - `/search` empty state explains *why* zero results happened in keyword-only mode and suggests clickable example queries

- **Config docs**
  - `api/.env.example` added (gitignored real `.env` not committed)
  - `Config::load()` calls `dotenvy::dotenv()` best-effort — works in dev, prod keeps real env-vars

## 2026-05-25 — Artwork detail vertical slice

- **Rust API**
  - `GET /v1/artworks/:id` — full artwork response (title, description, year, medium, dimensions, price, availability, external_url, published_at, embedded artist summary, ordered images)
  - `GET /v1/artworks/:id/similar?limit=8&include_same_artist=false` — pgvector cosine kNN against the anchor's own embedding, excluding the artwork itself and (by default) other works by the same artist
  - New `core::models` types: `ArtworkFull`, `ArtworkArtist`, `ArtworkImage`
  - Aligned `EMBEDDING_MODEL_VERSION` default to `"local"` (matches what the Python seed wrote). `T-024` in TODO tracks unifying the label before HTTP-API embeddings go live in prod.

- **Next.js**
  - `/artworks/[id]` server-rendered page: 7/5 desktop grid (image left, details right), stacked on mobile, additional images strip, "More like this" row
  - Details panel: artist credit (linking to portfolio), year, medium, dimensions (formatted from JSON), availability, price, description, disabled CTAs for Inquire + Save (deferred to T-009 / T-013)
  - Typed client: `getArtwork`, `getSimilarArtworks`, `formatDimensions`
  - 404 propagation via `notFound()` for missing artwork

- **Decisions log entry**
  - 2026-05-25 — Artwork detail: full-page first, modal-overlay deferred (intercepting routes are buggy in edge cases; modal is UX polish, not a feature)

## 2026-05-25 — First vertical slice (search + artist pages)

- **Rust API**
  - Hybrid search ranking on `/v1/search`: keyword tsvector + vector embedding via Postgres-backed `query_embedding_cache`, fused via RRF (k=60), with structured filters (medium, price, availability, location, near_lat/lng/radius_km) and sort options (relevance / newest / price / nearest)
  - Geographic filter: ILIKE on `coalesce(city, country)`; Haversine distance for `near_*` params
  - New endpoint `GET /v1/artists/:slug` — profile + first 24 artworks + 3 representative image URLs
  - Graceful degrade: no `JINA_API_KEY` → keyword-only search
  - Bugs caught and fixed: `$60` format-string footgun, NUMERIC vs FLOAT8 cast, sqlx 0.9 `AssertSqlSafe` requirement, owned-value bind requirement, axum 0.7 path syntax `:slug` (not `{slug}`)

- **Next.js skeleton** (`web/`)
  - `pnpm create next-app` with TS + Tailwind v4 + app router + Turbopack
  - Custom palette: gallery off-white `#FAFAF8`, near-black `#1A1A1A`, light-mode only
  - Pages: `/` (hero search + recently-added grid), `/search?q=...&location=...`, `/artists/[slug]`
  - Components: `TopNav`, `SearchBar` (hero/nav variants), `ArtworkCard` (artist name as separate link), `ArtworkGrid`
  - `lib/api.ts` typed client mirrors `core::models`
  - pnpm 11 quirks resolved: `allowBuilds` in `pnpm-workspace.yaml`, `verify-deps-before-run=false` in `.npmrc`

- **End-to-end verified**
  - Search by keyword: `/search?q=ukiyo` → real Ukiyo-e results
  - Filter by location: `/search?location=berlin` → Berlin-assigned styles
  - Artist portfolio: `/artists/demo-ukiyo-e` → header + bio + 24 works
  - 404 propagation: unknown slug → `notFound()` in Next.js
  - RFC 7807 errors: `sort=nearest` without coords → problem+json 400

## 2026-05-25 — Rust API scaffold and docs

- **Rust API workspace** in `api/`
  - Workspace + `core` crate (config, db pool, embedder, error, models, telemetry)
  - First binary `api-search` with `/v1/health` + stub `/v1/search`
  - Same handlers run as Lambda (`AWS_LAMBDA_RUNTIME_API`) or local Axum HTTP server
  - End-to-end verified: `curl localhost:9100/v1/search` returns seeded artworks with MinIO image URLs that serve 200 OK

- **Stack-decision tweaks**
  - sqlx bumped 0.8 → 0.9 (pgvector 0.4 transitive requirement)
  - Rust toolchain pinned 1.95
  - `cargo check`/`cargo run` clean

- **Documentation**
  - `decisions.md` — chronological decision log, 11 entries backfilled
  - `CHANGELOG.md` — engineering-facing log of state changes
  - `COST.md` — free-tier audit, $20/mo budget cap, env-var kill switches per service
  - `STRATEGY.md` — open non-engineering tracks (outreach, brand, legal preflight, build order)
  - `TODO.md` — open engineering items with priority/context

- **Schema**
  - `0008_query_cache.sql`: Postgres-backed text query embedding cache; replaces a Redis dependency at v1

## 2026-05-25 — Foundation infra and demo seed

- **Foundation infra**
  - `docker-compose.dev.yml`: Postgres+pgvector (port 5433), MinIO (9000/9001), Mailhog (SMTP 2025 / UI 8025)
  - `db/migrations/` 0001–0007: 19 tables covering users, artists, artworks, embeddings, collections, neighborhoods, inquiries, uploads, events, profiles, ML artifacts, eval set
  - Geographic v1 added to schema: `city`, `country`, `lat`, `lng`, `geocoded_at` on `artists` + matching indexes
  - `ml/ml_art/seed.py`: ingests WikiArt corpus into Postgres + MinIO + embeddings
  - End-to-end smoke test: pgvector k-NN returns same-style nearest neighbors

- **WikiArt corpus**
  - `ml/ml_art/datasets/wikiart.py`: stratified streaming sampler
  - Fetched 2000 images covering 27 styles to `ml/spikes/2026-05-modifier-deltas/data/wikiart/`

- **Modifier-delta spike, round 2 (rigorous)**
  - Re-ran on WikiArt corpus; verdict reversed to *ship*
  - Sweet spot α=0.8; α=0.4 is too weak, α=1.2 over-shifts
  - Findings in `ml/spikes/2026-05-modifier-deltas/FINDINGS.md`

- **Local embedder**
  - `ml/ml_art/embeddings/local_jina.py`: jina-clip-v2 via transformers
  - Parallel PIL decode via `ThreadPoolExecutor`, batched MPS forward
  - Decode: ~1400 imgs/sec; embed: ~14.5s/batch-of-32 on MPS

- **Spec updates** (see `01..05` + `99-deferred.md`)
  - Defer pre-built portfolio claim flow (legal risk)
  - Defer admin submissions UI
  - Switch to all-AWS infra (OpenNext + Rust Lambdas + Terraform)
  - Add geographic minimal to v1, Spaces+Events to v2/v3
  - Document the kill/pivot metric for monetization gating

- **Process**
  - `decisions.md`: backfilled 11 significant decisions
  - This file: `CHANGELOG.md`

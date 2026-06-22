# Changelog

Engineering-facing log of what shipped, in date order. Strategic / architectural
rationale lives in `decisions.md`.

## 2026-06-22 — T-070: Studio artwork-dimensions editor + search size-band filter

Artists couldn't enter physical artwork dimensions from the studio,
which meant most works had NULL `artworks.dimensions` and the planned
size filter on `/search` had no data to filter on. Surfaced during my
own real-artist test of the onboarding + studio flow.

Backend:
- `core::validation::dimensions_v1` — closed-schema validator for
  `{"unit":"cm","width","height","depth"?}`. Width + height required
  when the object is present (all-or-nothing); 1 ≤ n ≤ 5000cm; unit
  defaults to "cm" on output; unknown keys rejected. 19 unit tests.
- `studio::artworks::{create,patch}` call the validator before
  binding to SQL — invalid shapes 400 with a field-level message,
  absent dimensions pass through unchanged.
- `/v1/search?size=s|m|l` — bands over `GREATEST((dimensions->>'width')::int, (dimensions->>'height')::int)`:
  S ≤ 40, M 41–100, L > 100. Non-dimensioned works silently
  excluded; unknown band values fall through with no clause
  (tolerant of future renames in bookmarked URLs).
- 8 new integration tests cover band selection, exclusion of
  undimensioned works, unknown-band tolerance, and the validator's
  persist + normalise + reject paths.

Web:
- `ArtworkEditModal` gains a 3-input row (width / height / depth-cm)
  with mirrored client-side validation + inline error rendering.
- Soft `window.confirm` fires on the draft→published transition
  when dimensions are missing — "buyers won't be able to filter by
  size; publish anyway?". Non-blocking — just a nudge.
- `FilterBar` gains a `size` pill alongside medium / price /
  availability / location. `SIZE_BANDS` constant in `lib/filterBar.ts`
  drives both the pill and any future surfaces (e.g. neighborhoods,
  for-you).

Decisions: `decisions.md` 2026-06-22 captures the cm-only call, the
3-bands-vs-5 trade, single-band-per-query, longest-side determinant,
and dimensions-stay-optional-at-every-status (with the alternatives
considered + reversibility ratings).

## 2026-06-22 — Image pixel-dimension probe on upload

Every `/v1/uploads/image` PUT was leaving `uploads.width` + `.height`
NULL because nothing was probing the bytes. Downstream `artwork_images`
inherited that — public artwork pages and OG cards had no aspect-ratio
reservation, so each image load caused layout shift.

Wired a single trustworthy probe at the api boundary:

- Migration `0020_uploads_image_dimensions.sql` — adds nullable
  `width` / `height` to `uploads` (legacy rows stay valid; NULL means
  "we don't know" and downstream falls back gracefully).
- `core::images::probe_image_dimensions(&[u8]) -> Option<(u32, u32)>`
  — header-only read via the `imagesize` crate (~50 bytes per call,
  no full decode). Three unit tests covering PNG header, truncated,
  garbage.
- `/v1/uploads/image` probes between embed and S3 PUT, stamps the
  upload row.
- `POST /v1/studio/artworks/:id/images` prefers the dims from the
  `uploads` row when the s3_key starts with `uploads/` — single
  source of truth, client can't spoof. Falls back to body for
  non-upload s3_keys (e.g. seed-bucket `demo/` paths).
- Two new integration tests cover both branches of the attach handler.

Wires the data side of `T-062`'s eventual size filter, but the
artist-facing **physical**-dimensions editor is still missing — logged
as `T-070`.

## 2026-06-22 — T-054 productionisation: WAF tuning, Worker UA, S3 + STS fixes

T-054 shipped the night before but the first real prod cutover surfaced
five separate blockers, each in different infra/runtime layers. Each
fix is small; the rollup matters.

- **AWS WAF body-content rules vs image uploads + JSON webhooks.**
  AWSManagedRulesCommonRuleSet's body inspection (8KB cap +
  XSS/LFI/RFI/SSRF heuristics) blocked legitimate traffic on both
  CloudFront distributions. Adobe XMP in PNG metadata reads as XSS;
  random image bytes match LFI/RFI; multipart bodies exceed 8KB.
  Demoted 5 sub-rules to COUNT on both web + api ACLs
  (`SizeRestrictions_BODY`, `CrossSiteScripting_BODY`,
  `GenericLFI_BODY`, `GenericRFI_BODY`, `EC2MetaDataSSRF_BODY`).
  Rationale in `decisions.md` 2026-06-22.

- **WAF logging into Terraform.** Flipped on out-of-band via CLI
  during debug; adopted both `aws-waf-logs-ml-art-prod-{web,api}`
  log groups + `aws_wafv2_web_acl_logging_configuration` resources
  into TF state via `terraform import` so a clean apply still has
  them. 3-day retention (triage-only).

- **`NoUserAgent_HEADER` blocked the Worker → webhook POST.**
  Cloudflare Workers' `fetch()` doesn't add a default `User-Agent`,
  so AWS WAF (still on the Common Rule Set, this sub-rule still
  enforced) was 403'ing every inbound-email Worker call to the api
  before it reached the Lambda. Added a `User-Agent` to the Worker
  fetch; left the WAF rule enforced (most no-UA traffic IS bot
  scanning — fix belongs on the legit caller). Worker also gained
  structured try/catch + console.log/error so future failures
  surface in `wrangler tail` without needing a debug-echo round-trip.

- **`S3_UPLOADS_BUCKET` env var missing on api + jobs Lambdas.** The
  Rust config defaults to the literal string `"uploads"` when the var
  is unset — no such bucket in prod, so every PUT 500'd. Threaded the
  bucket NAME (not ARN) through `modules/storage` → `main.tf` →
  `modules/{api,jobs}` and set the env in both Lambda config blocks.

- **STS-credentials override in `core::object_store`.** Static-cred
  override fired unconditionally when `AWS_ACCESS_KEY_ID` +
  `AWS_SECRET_ACCESS_KEY` env vars were present. Lambda runtime
  auto-injects those AND `AWS_SESSION_TOKEN` from the role's STS
  session — by passing the first two through `Credentials::new(ak,
  sk, None /*token*/, ...)`, we created a broken static-cred override
  S3 rejected as AccessDenied (collapsed to "service error" by the
  SDK's `Display` impl). Gated the override on `endpoint_url.is_some()`
  — MinIO-only. Also switched the error formatter to `{e:?}` so the
  full SDK error chain reaches CloudWatch next time.

Net: studio image uploads + inquirer-inbound replies both now work end
-to-end against the live stack. Cosmetic spacing fix on the inquiry-
modal ack copy rode along.

## 2026-06-21 — T-054: Inquirer-inbound replies (email-stitched threads)

Closes the inquiry conversation loop. Previously: anonymous user
inquires → verifies email → artist replies from studio → email
delivered to inquirer. The inquirer hitting Reply went into a black
hole because the Reply-To was the platform's `info@wander.gallery`.

Now: artist-reply email's Reply-To is `r-<inquiry_id>-<hmac>@reply.wander.gallery`
— a tokenised per-inquiry address. When the inquirer hits Reply, mail
routes through Cloudflare Email Routing MX → an Email Worker that
parses with `postal-mime` and strips quoted history → POSTs the
extracted text + token to `/v1/webhooks/email/inbound` on the api. The
webhook verifies the HMAC, persists a new `inquiry_replies` row with
`from_role='inquirer'`, and enqueues a forward-to-artist email.

Schema (migration `0019_inquiry_inbound_replies.sql`):
- `inquiry_replies.from_role text NOT NULL DEFAULT 'artist'` so the
  existing studio-reply INSERT keeps working unchanged.
- `inquiry_replies.artist_id` becomes nullable — inquirer-authored
  rows have no artist author.
- `inquiry_replies.inbound_message_id text` + partial unique index —
  the replay-dedup key, populated by the webhook from Gmail's
  `Message-ID` header.

Code:
- New `core::reply_address` — mint + constant-time verify of the
  55-char local part (under RFC 5321's 64-char limit; truncated
  HMAC-SHA256 over the inquiry UUID with the same `anon_cookie_secret`
  reused across the app's HMAC needs). Seven unit tests pin shape,
  domain separation, tamper-rejection.
- New `api-search::webhooks::inbound_email` — closed-by-default auth
  (constant-time secret compare), HMAC verify, idempotent INSERT
  (`ON CONFLICT (inbound_message_id) DO NOTHING RETURNING id`), forward
  enqueue with idempotency key.
- New `JobEvent::InquirySendReplyForward` + handler that emails the
  artist with the inquirer's address as Reply-To so the artist can
  respond (currently via email only — capturing artist-side
  email-replies into the thread is deferred).
- `studio/inquiries.rs::list` returns `from_role` per reply so
  `<InquiryInbox>` can render with role-aware labels + accent border.

Infra:
- `modules/dns/email_routing.tf` — Cloudflare MX (route1/2/3) + SPF
  for `reply.wander.gallery`. Priorities are zone-assigned;
  `allow_overwrite = true` lets TF adopt whatever Cloudflare generated
  on enable.
- `infra/email-worker/` — Cloudflare Worker with `postal-mime` for
  MIME parsing, quoted-history trimmer (Gmail / Apple Mail / Outlook
  attribution patterns), `workers_dev = false` (Email Routing is the
  ONLY entrypoint).
- `INBOUND_EMAIL_SECRET` added to SSM SecureString placeholders;
  `Config::load` bails in prod if unset; same value also lives as a
  Cloudflare Worker secret.

Tests:
- 4 integration tests on the webhook (happy path threads + enqueues;
  tampered token rejected; missing/wrong secret 401; replay-same-id
  idempotent).
- 2 jobs-handler tests on the email path (artist reply uses tokenised
  Reply-To; forward emails the artist with inquirer's address as
  Reply-To).
- 7 unit tests on `core::reply_address`.

Post-deploy steps (POST_DEPLOY.md step 7) — set
`inbound_email_secret`, enable Email Routing on the subdomain via the
Cloudflare dashboard (not Terraformable), `wrangler secret put
INBOUND_SECRET`, `wrangler deploy`, bind the catch-all routing rule →
Worker, smoke-test.

## 2026-06-20 — T-052c: anonymous follow queueing

The "click Follow → bounce to sign-in → lose the click" funnel leak is
closed. When a signed-out user clicks Follow on an artist page, the
intent is now recorded against the signed `X-Anonymous-Id` cookie via
a new `POST /v1/anon/pending/follows/:artist_id` endpoint, before the
redirect to `/sign-in`. After sign-in, the existing T-033
`merge-anonymous` handler drains the queue and replays each intent as
a real `follows` row.

Schema (migration `0018_anon_pending_actions.sql`):
- `anon_pending_actions (id, anon_id, kind, payload jsonb, created_at,
  expires_at default now() + interval '7 days')`.
- Composite unique index on `(anon_id, kind, payload)` for dedup —
  double-clicking Follow doesn't queue twice.
- Generic shape (`kind text`) so future intents (save-to-collection,
  inquiry-start) plug in by adding a `kind` value, not a new table.

Why a server-side table vs cookie storage: see `decisions.md`
2026-06-20 — generalises beyond Follow, keeps the cookie minimal,
auditable / debuggable, lets the merge handler drain in the same
transaction it already uses for `uploads` + `events`.

Endpoint behaviour: 400 with no `X-Anonymous-Id` header (we need a
key to queue against); 404 on unknown / soft-deleted artist; 204 on
success; per-anon cap of 50 pending intents (BadRequest above cap
with copy that points the user at signing in to flush).

`MergeResponse` gains `follows_replayed: u64`. The drain deletes
*all* rows for the anon_id post-replay — recognised + unknown kinds,
valid + expired — so a stale intent never fires later when this code
learns a new kind name.

Web side: `<FollowButton>` signed-out branch wraps the redirect in a
`startTransition` that first calls `queueAnonFollowAction`
(server action → API endpoint via the cookie's anon-id).
Queue failures log to Sentry but never block the redirect; the user
can always click Follow again after signing in.

8 integration tests in `anon_pending_test.rs`. Prod-verified
end-to-end.

## 2026-06-20 — T-052b: new-works digest pipeline live

Daily cron-driven email digest of new artworks from artists each user
follows. EventBridge → SQS → kickoff handler → per-user fan-out →
batched email. Turns the follow graph (T-052 Phase 1, shipped
2026-06-18) into the actual retention loop.

Schema (`0017_user_notification_log.sql`): `(user_id, kind, sent_on
date, sent_at, context jsonb)` with composite PK that includes
`sent_on` for daily-cadence dedup via `INSERT … ON CONFLICT DO
NOTHING RETURNING`. Survives SQS at-least-once redeliveries cleanly.

Backfill semantics use `follows.created_at` per-follow + a 24h floor
(`a.published_at > GREATEST(f.created_at, now() - 24h)`) — no
`notifications_started_at` column needed. New follow today never
backfills the artist's archive; existing follow at launch only sees
future artworks; the floor caps the lookback so one cron can't dump
a week of works in one go.

`JobEvent::NotifyFollowersDigestKickoff` (cron entry point) +
`NotifyFollowersDigestUser { user_id }` (fan-out target).
`EmailClient::send_notification` wraps `send` adding `List-Unsubscribe:
<url>, <mailto:>` + `List-Unsubscribe-Post: List-Unsubscribe=
One-Click` headers per RFC 8058 so Gmail/Outlook honour the URL
for one-click. Sender-reputation boost as a bonus.

Local-dev trigger: `cargo run -p jobs-worker -- --enqueue '<json>'`
drops any event into the local Postgres jobs table; the worker loop
picks it up next poll.

11 integration tests covering kickoff (positive, no-new-work,
already-sent-today, master kill switch, per-kind opt-out, per-follow
backfill window) and per-user handler (email send, idempotency,
defensive opt-out re-check, multi-artist subject, empty-payload
silence).

Prod-verified end-to-end: manual SQS enqueue → kickoff returned
`candidates: 0, enqueued: 0` (correct empty-state — no follows with
new artworks in the 24h window yet).

## 2026-06-20 — T-068: notification preferences spine

The infrastructure layer every notification-emitting feature
(T-052b new-works, T-059 saved-search alerts, T-060 Discover Weekly,
future artist-side digests) composes on top of. Future kinds just
add one `NotificationKind` enum variant + one row in the settings UI.

Schema (`0016_notification_preferences.sql`): per-(user, kind)
override table with composite PK, partial index on enabled rows for
cron-side "everyone opted into kind X" lookups, and a master
`users.global_email_notifications_enabled` kill switch. Default-on
implicit (no row = enabled), so most users will have zero rows.

Three-class taxonomy in `core::notifications::NotificationKind`:
- Transactional (InquiryVerification, InquiryReply): sent regardless
  of preferences. `is_transactional()` short-circuits the user_wants
  check. Required for service operation; CAN-SPAM / CASL / GDPR
  exempt from opt-in.
- Notifications (NewWorksDigest, …): user-controllable, default-on.
- Marketing / product: reserved slot, none today; would need
  explicit opt-in.

`core::notifications::user_wants(pool, user_id, kind)` is the single
chokepoint every notification-email-sending handler routes through.

JWT-based unsubscribe tokens (HS256, jsonwebtoken — already a
dep, no new crates). Signed with the existing `ANON_COOKIE_SECRET`;
90-day TTL.

API: `GET`/`PATCH /v1/me/notification-preferences` (sparse partial
updates with kind-name validation) + `POST /v1/notifications/
unsubscribe` (no-auth, signed token IS the credential) + `POST
.../oneclick` (returns 204 for RFC 8058 mail-client one-click).

Web: `/me/settings` index (future home for account, privacy, data
export sections), `/me/settings/notifications` with optimistic
toggles, `/u/[token]` route handling GET (→ `/u/confirm`
landing page) + POST (mail-client one-click). UserButton dropdown
gains a "Settings" link via Clerk's `<UserButton.MenuItems>`.

20 tests in total (7 unit + 13 integration).

## 2026-06-20 — Infra cleanup: KMS spend + Neon connection risk

Two cleanups before more feature work.

**Postgres → Neon pooler endpoint.** `database_url` SSM value updated
from the direct endpoint to the pooled one (`-pooler` suffix). With
`max_connections(8)` per Lambda × many warm Lambda instances under
burst, the direct endpoint risked exhausting Neon's connection limit.
The pooled endpoint sits in front of the same database but
multiplexes via PgBouncer-style transaction pooling. No code change.

**Non-secret SSM params → TF-managed Lambda env vars.** Six SSM
parameters (`clerk_issuer`, `clerk_jwks_url`, `web_base_url`,
`image_base_url`, `uploads_public_url_prefix`, `resend_from_email`)
used to live as SecureString in SSM, burning a KMS Decrypt call per
parameter per cold start for no security benefit (their values are
either public URLs or our own sender email). Moved them to TF-managed
env vars on api + jobs Lambdas. `core::config::bootstrap_ssm` still
loads the remaining 8 real secrets.

  KMS Decrypt per cold start: 14 → 8 (~43% reduction).
  SSM params: 14 → 8.

Both wins on their own put us comfortably under the 20k/month KMS
Free Tier ceiling we were tracking toward breaching mid-June.

## 2026-06-19 — Clerk auth fixed at API Gateway

Resolved a multi-day debugging arc: signed-in users hitting
`wander.gallery/` got `redirect_url is invalid` (HTTP 422) from
Clerk's frontend API.

Root cause: CloudFront uses `AllViewerExceptHostHeader` origin
policy (required so API Gateway's SNI matches the cert on the API
Gateway endpoint). That strips the original viewer Host and sends
API Gateway's own hostname. The Lambda saw `Host:
afttdiav9e.execute-api.…amazonaws.com`, Clerk's middleware built
every absolute URL from that wrong hostname, and Clerk's allowed-
origins list (correctly) didn't include the API Gateway URL.

Fixed by API Gateway parameter mapping at the integration → Lambda
boundary: `request_parameters = { "overwrite:header.Host" =
"wander.gallery" }`. Three lines of Terraform; the Lambda now
receives Host = canonical hostname natively and Clerk's URL
generation works. No DNS work, no custom domain, no middleware
reconstruction. See `decisions.md` 2026-06-19 for the architectural
rationale + the two paths we tried and abandoned (custom domain
choreography; middleware request reconstruction that hung the
Lambda at 10s).

Adjacent fix: middleware now 308-redirects direct hits to the API
Gateway invoke URL back to `wander.gallery` (URL-healing for stale
bookmarks + autocomplete entries). Detection via `X-Amz-Cf-Id`
absence (we can't use Host now that it's rewritten). Full lockdown
of the API Gateway URL behind a CloudFront shared-secret header is
tracked as `T-064`.

`<ClerkProvider>` got explicit relative URLs
(`signInUrl="/sign-in"`, `signInFallbackRedirectUrl="/"`, etc.) as
belt-and-braces — the underlying bug is fixed but pinning these
means a future proxy-chain change can't silently reintroduce a
bad-redirect bug.

## 2026-06-19 — Never render raw `Error.message` to users

Reported bug: the Save-to-collection modal surfaced Next.js's
internal Server Components verbiage ("An error occurred in the
Server Components render. The specific message is omitted in
production builds to avoid leaking sensitive details…") in place of
friendly copy. Same pattern was in 14 other client catch blocks.

Added `toUserMessage(err, fallback, context)` helper in
`lib/reportError.ts` — sends the raw error to the reporter
(CloudWatch, future Sentry) and returns the supplied fallback
string. Swept every `setError(e instanceof Error ? e.message : ...)`
site. Users see neutral, actionable copy regardless of what the
framework produced.

## 2026-06-18 — T-052 Phase 1: follow-an-artist

Single biggest retention hook missing. Visible product surface
(button + count) of the follow graph; the notification engine that
turns it into recurring engagement landed as T-052b two days later.

Schema: `follows (user_id, artist_id, created_at)` composite PK +
reverse `(artist_id, created_at DESC)` index for "who follows this
artist."

API: `POST`/`DELETE /v1/me/follows/:artist_id` (both idempotent —
double-clicks and rapid follow/unfollow cycles stay clean); `GET
/v1/me/follows` (paginated, with slug + display_name + city/country
+ first published thumb). `ArtistDetail` gains `is_following`
(auth-conditional via `Option<AuthedUser>`) + `follower_count`.
`GET /v1/studio/me` is now a `StudioMe` wrapper that flattens the
artist row + `follower_count`; wire-compatible for every existing
field via `#[serde(flatten)]`.

Web: `<FollowButton>` on `/artists/[slug]` with optimistic flip;
signed-out branch redirects to sign-in (anon-side queueing landed
in T-052c two days later); follower-count pill when > 0; same pill
on the `/studio` dashboard.

9 integration tests.

## 2026-06-18 — T-053: shareable collections (public read)

Schema was already there from migration 0003 (`user_collections.is_
public` + `share_id`). UI didn't expose them. Added:

- API `GET /v1/collections/share/:share_id` — unauthenticated read.
  Returns `CollectionDetail` if public; 404 indistinguishably for
  not-found / private / soft-deleted. Cheap pre-DB guard rejects
  malformed tokens (length + alphanumeric) before hitting the query.
- Factored `fetch_collection_artworks` out of the owner-side detail
  handler so both surfaces share filtering (published + active +
  approved primary image).
- Web `/c/[share_id]` public read-only view + per-collection OG card
  (composes the same 2×2 cover-grid pattern as T-051).
- `<CollectionShareControl>` on `/collections/[id]`: "Make public"
  toggle → server action → renders the share URL + Copy button +
  "Make private" toggle. UI is explicit that going private rotates
  the link (the existing PATCH handler mints a fresh `share_id`
  on every false→true transition; old shares stay dead).
- Privacy page acknowledges that public collections may be indexed
  by search engines once shared.

5 new integration tests (19 collections total).

## 2026-06-18 — T-051: per-artwork + per-artist OG cards

Every share of `/artworks/<id>` or `/artists/<slug>` (iMessage,
Slack, WhatsApp, Twitter, etc.) now pulls the actual work into the
preview, not the generic homepage card. Free distribution.

Both routes use Next.js `ImageResponse` (Satori + Resvg WASM) at
request time; 1200×630 PNG; revalidate every 24h.

The OpenNext gotcha worth recording: the Next.js-docs pattern
`fetch(new URL('./font.ttf', import.meta.url))` does NOT work under
vanilla Node — Vercel's edge runtime supports `fetch('file://…')`
but Node's undici throws "not implemented... yet..." on the file:
scheme. Fix is `readFile(fileURLToPath(new URL(…, import.meta.url)))`.
Turbopack bundles the relative TTFs into `.next/server/assets/`
either way; only the load path changes. Documented inline in both
route files so the next person doesn't reach for `fetch` first.

Fonts (Instrument Serif regular + italic) bundled at
`web/src/app/og-fonts/`. Title size clamps by length so a 60-char
title doesn't blow the box.

## 2026-06-17 — Roadmap track: post-launch retention + ML

Strategic session promoted 14 new tracks from speculative to "next
3-month build queue":

- T-050 events writer (foundation for taste vector + analytics)
- T-051 OG cards · T-052 follow-an-artist · T-053 shareable
  collections (Tier 1 retention)
- T-054 inquirer-inbound replies (closes inquiry-reply thread loop)
- T-055 taste vector + nightly refresh · T-056 personalised search
  re-rank + "For you" row · T-057 algorithmic neighbourhoods
  (HDBSCAN + Claude label) (ML core)
- T-058 series concept for artists · T-059 saved searches · T-060
  Discover Weekly · T-061 first-session taste calibrator · T-062
  size/price/medium filters · T-063 inline "more like this" (UX
  + retention)

Plus four `decisions.md` entries recording the underlying positions:
no in-platform messaging (inquiries are email-stitched); ML-driven
discovery (no editorial); algorithmic neighbourhoods as primary
primitive; Postgres-hot / S3-cold event storage.

## 2026-06-11 — Web is live: Next.js SSR via OpenNext on Lambda

`https://wander.gallery` now serves the real Next.js app — full SSR,
prod Clerk publishable key, prod API endpoint baked into the client
bundle. All three Lambdas (api, jobs, web) are running real code.

What ships in the bundle:
- React Server Components compiled by Next 16.2.6
- Clerk auth UI loading from `clerk.wander.gallery` (custom domain
  via the CNAMEs we added earlier)
- API client pointing at `https://api.wander.gallery`
- Mapbox client token wired for the map mode
- Static assets (`/_next/static/*`, fonts, public/) served from
  the S3 web-assets bucket via CloudFront, immutable cache

Footguns hit + fixed:

1. **OpenNext 4.0 supports Next 16.2.6+** at the lower bound exactly.
   Worked first try once we confirmed compat.
2. **pnpm Node version mismatch.** Local Node is 20.19 (via nvm);
   pnpm 11 requires 22.13+. Used Homebrew's Node 23 by pinning PATH
   in deploy-web.sh (`PATH=/opt/homebrew/opt/node/bin:$PATH`).
3. **OpenNext bundler stripped @swc/helpers/cjs/* + @next/env.**
   Next's compiled output requires them at runtime but the bundler
   doesn't see the deep imports through pnpm's symlinked store.
   Fixes layered:
   - `web/.npmrc` `node-linker=hoisted` — flat node_modules
   - `pnpm add @swc/helpers @next/env@16.2.6` — direct deps so
     they're top-level visible
   - `next.config.ts` `outputFileTracingIncludes` for both pkgs —
     forces Next to include the trees even when not auto-detected
4. **`.env.production` was overridden by `.env.local`.** Next's
   load order is `.env.production.local` > `.env.local` > `.env.production`.
   Renamed to `.env.production.local` so prod values win for prod
   builds without touching local dev.
5. **70MB Lambda zip ceiling.** Initial broad include
   (`node_modules/next/dist/**`) ballooned to 161 MB. Narrowed to
   `@swc/helpers/**` + `@next/env/**` only — back to 8 MB.
6. **`update-function-configuration` vs `update-function-code`
   race.** Lambda rejects the second call while the first is still
   in `InProgress`. Added `aws lambda wait function-updated` between
   them.
7. **OpenNext output path is `.open-next/server-functions/default/`,
   not `.open-next/server-function/`** as the original deploy script
   assumed. Updated.
8. **Server-side env injection** (CLERK_SECRET_KEY etc) is done by
   `deploy-web.sh` via `aws lambda update-function-configuration`
   reading from SSM. Keeps secrets out of TF state; rotation =
   update SSM + re-deploy.

`make deploy-web` now runs end-to-end in ~2 min: build, zip, code
upload, env sync, asset s3 sync, CloudFront invalidation, smoke test.

## 2026-06-11 — Real Rust Lambdas live: api-search + jobs-lambda

First successful prod deploy of real code. End-to-end paths:

- **api-search** — `https://api.wander.gallery/v1/health` returns
  `{"auth_enabled":true,"db":true,"embedder_enabled":true,"env":"Dev","status":"ok"}`.
  Connected to Neon (us-east-1), reads Clerk JWKS, embedder client built.
- **jobs-lambda** — SQS `send-message` → cold-start 1020ms → handler
  runs in 5ms warm. Test event consumed cleanly.

Two things made this work end-to-end:

1. **`core::config::bootstrap_ssm(prefix)`** — fetches every SecureString
   under `/ml-art-prod/` at cold start (one `GetParametersByPath`),
   injects each as an uppercased process env var. Existing
   `Config::load()` then reads via `env::var()` as before — no
   per-key plumbing needed for the 9 secrets. Called from both
   lambda mains before `Config::load`. `aws-sdk-ssm ~> 1.55` added
   to workspace.
2. **TF runtime swap** — flipped both `aws_lambda_function`s from the
   `nodejs20.x` placeholder to `provided.al2023` + `bootstrap` handler.
   Added `architectures = ["arm64"]` to the jobs module (api module
   already had it). One-time edit; subsequent `make deploy-{api,jobs}`
   only swaps code via `update-function-code` (TF `lifecycle.ignore_changes`
   keeps it from churning).

Footguns hit + fixed:
- jobs-lambda crate had `[[bin]] name = "bootstrap"` which made
  cargo-lambda write to `target/lambda/bootstrap/` (vs api-search's
  `target/lambda/api-search/`). Renamed bin to `jobs-lambda` for
  consistency; cargo-lambda always renames the final exe to
  `bootstrap` anyway because `provided.al2023` requires it.
- Default Lambda architecture is x86_64; the jobs TF was missing the
  explicit `architectures = ["arm64"]`, so the first invoke returned
  `cannot execute binary file`. Now wired and consistent across modules.

## 2026-06-10 — Deploy track: web deploy script + artworks-to-S3 sync

- `scripts/deploy-web.sh` + `make deploy-web` — builds via
  `opennextjs-aws build`, zips `server-function/`, pushes via
  `update-function-code --publish`, syncs `.open-next/assets/` to
  S3 with immutable cache headers, invalidates only the dynamic
  CloudFront paths (`/`, `/index.html`, `/api/*` — content-hashed
  static doesn't need it). Smoke-tests `https://wander.gallery` at
  the end.
- `scripts/sync-artworks-to-s3.sh` — mirrors the local WikiArt
  corpus into `s3://ml-art-prod-artworks/` so the images CDN starts
  returning real content. Defaults to dry-run; `--apply` does the
  copy. Idempotent + non-destructive (never deletes).
- `infra/POST_DEPLOY.md` — replaced the "TODO OpenNext not yet wired"
  placeholder with concrete install steps + a documented Next 16
  compatibility caveat (if OpenNext stable doesn't support 16,
  fallback is `@opennextjs/aws@beta` or pinning web to 15.x).

## 2026-06-10 — Deploy track: JobsBackend::Sqs driver + boot-time selector

Closes the last "real prod" gap in the API path. With the api-search
Lambda about to ship for real, enqueues from the request handlers
need to land on SQS (where `jobs-lambda` consumes them), not in the
local `jobs` table (where nothing in prod polls).

- `core::jobs::JobsBackend::sqs(client, queue_url)` constructor +
  `Inner::Sqs` variant + `core::jobs::sqs::enqueue()`. Sends the
  full tagged JobEvent JSON as the SQS message body, with a
  `kind` MessageAttribute mirroring the discriminator for cheap
  routing if we ever shard the queue.
- `Config::jobs_queue_url: Option<String>` — reads `JOBS_QUEUE_URL`
  env var. Already injected into the api Lambda's env by TF, so
  this just wires it through.
- `api-search/src/main.rs` picks driver at boot: queue URL present
  → SQS, absent → Postgres. Logs which path it took on init so
  CloudWatch shows it in the first line.
- New contract-pinning test in `core::jobs::tests`: serializes a
  JobEvent through the same `serde_json::to_string` path that
  `sqs::enqueue` uses, then deserializes through the same
  `from_value` path that `jobs-lambda` uses. Catches drift between
  producer and consumer before the first message DLQs in prod.
- `aws-sdk-sqs ~> 1.55` added to workspace + threaded through core
  and api-search.

What now flows end-to-end (in code; needs real deploy to verify):
- api Lambda receives a request → handler calls `jobs.enqueue(...)`
- → `JobsBackend::Sqs` → `aws_sdk_sqs::send_message`
- → SQS `ml-art-prod-jobs` queue
- → event-source-mapping (max 5/batch, max 10 concurrent)
- → `jobs-lambda` → `core::jobs::handle` → domain handler

## 2026-06-10 — Deploy track follow-up: jobs-lambda crate + deploy scripts + POST_DEPLOY runbook

Bridges the gap between "infra is up with placeholders" and "real
Rust code on Lambda."

- `api/crates/jobs-lambda/` — new crate. Single `bootstrap` binary,
  Lambda runtime (`provided.al2023`, ARM64), SQS-triggered. Reuses
  `core::jobs::handle` so domain handlers are identical to what the
  local `jobs-worker` polling loop runs. Returns
  `SqsBatchResponse { batch_item_failures }` so a single failing
  record in a batch of 5 retries only that one record (the other 4
  successes are deleted by SQS).
- `aws_lambda_events ~> 0.16` added to workspace deps (only the `sqs`
  feature — keeps build small).
- `scripts/deploy-api.sh` + `scripts/deploy-jobs.sh` — runnable
  build-and-deploy scripts. `cargo lambda build --release --arm64`
  → zip bootstrap → `aws lambda update-function-code --publish`
  → wait for active → smoke-test. `--check` flag for build-only
  (CI verification). Surfaced as `make deploy-api` / `make deploy-jobs`.
- `infra/POST_DEPLOY.md` — runbook for the steps TF can't automate:
  cargo-lambda install, Neon project creation, SSM secret population,
  first deploys, end-to-end smoke test, rotation hygiene, common
  failure-mode diagnostics. Linked from `infra/README.md`.

What's still TODO before launch:
- Neon project (out-of-band, per POST_DEPLOY step 1)
- Populate 9 SSM SecureString values (POST_DEPLOY step 2)
- Run `make deploy-api && make deploy-jobs` (needs cargo-lambda installed)
- Install OpenNext + write `scripts/deploy-web.sh`
- Seed `s3://ml-art-prod-artworks/` with the WikiArt demo corpus

## 2026-06-10 — Deploy track: infra is live on AWS

End-to-end TF scaffold + first apply against the prod AWS account.
After a couple of architectural pivots (see `decisions.md` 2026-06-10
entries), the stack is up and serving placeholder responses on real
TLS. Real code lands on top in follow-up commits.

### What's live

| URL | Backed by |
|---|---|
| `https://wander.gallery` | Web Lambda (Node 20 placeholder) → APIG → CloudFront, apex via Cloudflare CNAME-flatten |
| `https://api.wander.gallery` | API Lambda (placeholder) → APIG → CloudFront + WAF (rate-limit + AWS managed common rules) |
| `https://images.wander.gallery` | S3 (artworks + uploads, OAC-locked) → CloudFront. Buckets are empty, returns 403 until seeded. |

Plus, not user-facing:

- `ml-art-prod-jobs` SQS queue + DLQ → jobs Lambda (placeholder) via event-source-mapping with `max_concurrency = 10`
- SSM `/ml-art-prod/*` — 9 SecureString parameter containers (placeholders, populate out-of-band)
- AWS Budgets cap at $20/mo, 80% actual + 100% forecast alerts to `drjjm18@gmail.com`
- TF state in S3 `ml-art-tfstate` + DynamoDB lock table `ml-art-tfstate-lock`
- Cloudflare zone `c697407cceb224646ce6b13975956b2f` (`wander.gallery`) — DNS records managed via the `cloudflare/cloudflare` TF provider

### Resource count

72 AWS + Cloudflare resources, all green:

- `dns` — 3 ACM certs (us-east-1) + 4 Cloudflare validation CNAMEs + 3 validation barriers
- `secrets` — 9 SSM SecureString containers
- `storage` — 2 S3 buckets + lifecycle/versioning/encryption/policies, CloudFront, OAC, Cloudflare CNAME
- `jobs` — SQS + DLQ, Lambda, IAM role + policy, log group, event-source-mapping
- `api` — Lambda + IAM, APIG (api + integration + route + stage + permission), WAF v2, CloudFront, Cloudflare CNAME
- `web` — Lambda + IAM, APIG (same set), S3 web-assets + policy + OAC, CloudFront, Cloudflare apex CNAME
- root — `aws_budgets_budget.monthly`

### TF provider footprint

- `hashicorp/aws ~> 5.70` — everything in AWS
- `hashicorp/cloudflare ~> 4.40` — DNS records on Cloudflare's hosted zone (Registrar forces this; see decisions.md 2026-06-10)
- `hashicorp/archive ~> 2.4` — zips placeholder Node 20 lambda payloads inline (no committed binaries)
- `hashicorp/random ~> 3.6` — unused so far; kept for future password-shaped resources

### Operational pieces in place

- One-time backend bootstrap commands in `infra/README.md` (S3 versioned + encrypted + private; DynamoDB pay-per-request lock table)
- `terraform.tfvars` gitignored; `terraform.tfvars.example` committed
- All Lambda functions have `lifecycle.ignore_changes` on `filename`, `source_code_hash`, `environment` — CI replaces code via `aws lambda update-function-code` without TF reverting
- All 9 SSM params have `lifecycle.ignore_changes = [value]` — operator-set secrets persist across applies

### War stories (also in decisions.md)

1. **Cloudflare Registrar mandates Cloudflare nameservers.** Discovered mid-apply when trying to paste Route53 NS records at the registrar. Refactored: dropped `aws_route53_zone`, added the Cloudflare TF provider, kept ACM certs in AWS. Net change ~30 lines.

2. **New AWS accounts (~first few days) silently 403 Lambda Function URLs.** Spent ~30 min trying `auth_type=NONE` → `AWS_IAM` + CloudFront OAC → custom origin-request policy excluding `Authorization`. None worked; direct `aws lambda invoke` always succeeded. Pivoted to API Gateway HTTP API (v2) in front of both api + web Lambdas — the topology originally expected. APIG adds ~$0–1/mo + ~10-30ms latency; the wider compatibility wins.

### What's not done

- **Real Lambda code.** All three Lambdas (api, jobs, web) run a placeholder Node 20 zip. Next: `cargo lambda build` for api-search, write the `jobs-lambda` Rust crate, install OpenNext + build the web bundle. Each is a one-line `aws lambda update-function-code` after.
- **SSM secret values.** All 9 parameters hold `"placeholder — set out-of-band..."`. Populate before flipping real code on.
- **Neon DB.** Create the project, copy the connection string into SSM `/ml-art-prod/database_url`.
- **`www.<apex>` redirect.** ACM cert covers `www.wander.gallery` via SAN (cheap to have at issue-time); the redirect itself is a Phase-2 CloudFront Function.

## 2026-06-09 — T-011 Phase 5: bulk image upload in studio

Closes the last open piece of the artist studio. The image manager
inside `<ArtworkEditModal>` accepted one file at a time — an artist
with a portfolio of 30 pieces was stuck doing 30 individual picks.

- File `<input>` now has `multiple` and accepts a batch.
- `onFilesSelected` per-file-validates (image MIME, ≤10MB), drops
  bad files with a per-file note rather than failing the whole
  batch. Cap of 20 files per batch — protects against an accidental
  "select all" on a 500-photo folder.
- Sequential uploads through the existing `uploadArtworkImage`
  endpoint (no API changes). Each success appends to the image
  grid immediately so the user sees incremental progress; an
  early failure doesn't lose the prior successes.
- New "Uploading N of M" caption. Multi-line errors render with
  `whitespace-pre-line` so the per-file failures stack readably.
- No server changes — the bulk path reuses the per-image embed +
  moderation pipeline already in place from T-011 Phase 3.

## 2026-06-09 — Quick wins: T-008c (moderation reason in studio) + T-022 (demo prices/dimensions)

Two small UX gaps closed in one pass.

### T-008c — moderation rejection reason in studio

Until now the moderation handler computed rejection labels
("Explicit Nudity", "Violence", …) but only logged them; the
studio surface showed `rejected` images as plain "Remove" tiles
with no context, leaving the artist guessing.

- New `artwork_images.moderation_reason text` column (migration
  `0014_artwork_image_moderation_reason.sql`). Nullable: only set
  on rejection. Cleared if a re-run flips the row back to approved.
- `moderate_artwork_image` handler persists the comma-joined
  labels alongside the status flip.
- `StudioImage.moderation_reason` surfaced in the API response.
- `<ModerationBadge>` in `ArtworkEditModal`: amber "Checking…"
  while pending, red "Hidden · <labels>" when rejected (with
  full text in a tooltip for screen readers / hover). Rejected
  images dim + grayscale so it's visually obvious they're
  suppressed from public surfaces. Approved images get no badge.
- 1 new integration test asserting the reason is persisted;
  1 asserting it clears on re-approve. 8 total in the moderation
  suite (310 total Rust).

### T-022 — backfill demo prices + dimensions

The 2000 WikiArt demo artworks all had `NULL` `price_cents` and
`dimensions`, so the studio surface hid the price line entirely
and `<ArtworkCard>` never showed a price chip. Demo felt
unfinished.

- `seed.py` now sets both on INSERT via deterministic per-sha256
  helpers (`_demo_price_cents`, `_demo_dimensions`). Re-runs
  produce the same values. Currency stays at the schema default
  (`USD`). Prices quantised to nearest £10 (50–2500) so the UI
  shows tidy listings rather than uniform-random noise.
- One-off SQL backfill at `db/seeds/0001_demo_prices_dimensions.sql`
  for the already-inserted 2000 rows. Idempotent (only fills
  NULL cells, safe to re-run).

## 2026-06-09 — Search-resume UX: URL state restore + visual-search-from-artwork + map default

Three pieces that turn the search surface from "you can navigate, but
you lose your place" into "you can leave and come back exactly where
you were." Built deliberately URL-first — sessionStorage was tried,
removed in favour of the URL because the user-visible benefit is the
same and the URL gives us bookmarkability + shareability for free.

### State restore (URL-driven, no sessionStorage)

The /search page now encodes everything the user needs to "resume"
in the URL. Refresh, back-nav, paste-in-a-new-tab — all reproduce
the exact same view.

- **`?pages=N`** (default 1, cap 10). `page.tsx` loops cursor-chained
  fetches up to N, concatenates the results. Each Load More is
  `router.push('?pages=N+1', { scroll: false })` wrapped in
  `useTransition` (so the button's loading state is "free"). Server
  cost: N sequential `/v1/search` roundtrips per render — acceptable
  for v1 scale (≤ 240 items / ~1.5s p95).
- **`?focus=<artwork_id>`** — set on sidebar card click via
  `replaceState` (no re-render, no scroll jump). On mount, the
  matching artwork is found in `items`, `focusSignal` fires (map
  flies + popup opens) and the matching card `scrollIntoView`s.
  Cards carry a `data-artwork-id` attribute as the scroll anchor.
- **`<BackToSearchLink>`** — replaces the hard-coded
  `<Link href="/search">` on `/artists/[slug]` and `/artworks/[id]`.
  On click: `router.back()` when `document.referrer` is our own
  `/search` (full state restore via browser history), else `/search`
  push. Plain `<a href>` underneath so middle/cmd-click and
  no-JS / crawler traffic still works.
- **Removed** `lib/searchSnapshot.ts` + `lib/searchClient.ts`
  (sessionStorage approach + browser-side cursor client). Both
  dead code now that the URL is the source of truth.

The architectural call (URL vs sessionStorage) is captured in
`decisions.md` — search state belongs in the URL so every nav,
share, and refresh resolves to the same view. SessionStorage was
the wrong layer: it hid state in the browser, was un-shareable,
race-prone on hydration, and silently cleared on hot-reload.

### Visual search from a platform artwork

New `seed_artwork_id` param on `/v1/search`. Resolves the artwork's
existing CLIP embedding from `artwork_embeddings` — no upload
roundtrip. Precedence: `image_upload_id > seed_artwork_id > q`'s
text embed. The seed artwork itself is excluded from results (an
`AND a.id <> $seed` clause in `build_filters`) so the user doesn't
self-match at position 1. Modifiers now compose with either visual
anchor (loosened guard: was `image_upload_id`-only).

- **Web**: `getArtwork` server-side fetch on `/search` populates a
  new `<SeedAnchor>` strip with the seeded artwork's thumbnail +
  title + link back. `<ArtworkFull>`'s `images` walked for the
  `is_primary` thumbnail.
- **CTA**: "Find visually similar →" button on `/artworks/[id]`
  next to Inquire / Save. Routes to `/search?seed_artwork_id=<id>`
  so the user can layer modifiers + filters on top.
- **`describeQuery`** updated so the page heading reflects the
  seed source ("Results for a platform artwork").

### Map default — never the world view

`useFitToInitialPins` now (a) preserves the current viewport when
it already shows ≥ 1 pin from the new set (so "clear filter while
looking at London" stays in London), and (b) when it does refit,
uses the top-5 pins by relevance, not all of them. Same top-5 fit
applied to `useMapInstance`'s mount-time fit. The trailing pins
still render; they sit off-screen until the user pans.

### Performance: faster `flyTo`

`useFocusArtist` was using Mapbox's default scenic arc
(`speed: 1.2, curve: 1.4`) which takes 4–6 seconds from a global
view to a city pin — long enough that the bbox URL write (debounced
behind `moveend`) felt sluggish too. Bumped `speed → 2.0`, lowered
`curve → 1.1` (straighter line, less zoom-out), and capped with
`maxDuration: 1200`. Trip-then-write is now ~1.5s end-to-end.

## 2026-06-09 — T-011 Phase 4b: reply-from-inbox + auto-mark-as-read

Closes the biggest UX hole in the studio surface before onboarding a
real artist. The inbox shipped in Phase 4 was read-only — artists who
wanted to respond had to bounce out to their email client. They can
now reply directly from `/studio/inquiries` and the in-app history
captures the conversation thread.

- **New migration `0013_inquiry_replies.sql`** — adds an
  `inquiry_replies` table (one row per artist reply, ordered by
  `created_at`) and an `inquiries.read_at` column with a partial
  index over the unread set for the (eventual) unread-count badge.
  Modelled as a table rather than an `inquiries.reply_text` column
  so future threading lands without another migration.

- **API surface, three endpoints**:
  - `GET /v1/studio/inquiries` — extended to include `read_at` and
    a per-row `replies: [{id, message, created_at, sent_at}]`. The
    reply list is pulled in one follow-up query keyed on the page
    of inquiry ids (no N+1) and filtered by the same `artist_id`
    as the inquiries (ownership-safe even if ids were forged).
  - `POST /v1/studio/inquiries/:id/reply` — body `{ message }`.
    Validates ownership inline via a `WHERE artist_id = $caller`
    on the INSERT (avoids a TOCTOU SELECT-then-INSERT). Enqueues
    `JobEvent::InquirySendReply` with idempotency key
    `inquiry_reply:{id}:send`. 400 on empty / oversized message
    (`REPLY_MESSAGE_MAX_LEN = 4000` — matches the inquire side).
  - `POST /v1/studio/inquiries/read` — body `{ ids: [uuid] }`.
    Bulk mark-as-read with two safety nets: the SQL
    `read_at IS NULL` predicate makes re-marks no-op (idempotent),
    and the `WHERE artist_id = $caller` predicate silently drops
    cross-artist ids rather than 403'ing. Hard cap of 100 ids per
    request — protects against a malicious or buggy client.

- **`JobEvent::InquirySendReply { reply_id }`** + dispatch +
  `inquiry_handlers::send_reply` handler. Loads the reply +
  surrounding context, sends via Resend with `reply_to = artist's email`
  so the inquirer's reply bounces back to the artist's inbox.
  Idempotent on `inquiry_replies.sent_at`: a second invocation
  sees the timestamp and returns Ok without re-sending. New
  `templates::artist_reply` mirrors the existing email shape.

- **Web UI** — `/studio/inquiries` page stays a server component
  for the auth + initial fetch; the actual rows + reply forms move
  into a new client component `<InquiryInbox>`:
  - per-card collapsible reply form with textarea + Send button,
  - optimistic append of the freshly-created reply (no re-fetch),
  - auto-fired `POST /api/studio/inquiries/read` on mount with
    whatever was unread (best-effort; silent on failure).

  Two route handlers bridge `apiFetch` → API so the client
  component can use plain `fetch` without dragging the server-only
  Clerk module into the browser bundle.

- **7 new integration tests** (16 total on the inbox suite, 299
  total Rust): reply persist + appears-on-list, reply ownership
  404, reply empty-message 400, reply unknown-id 404, mark-read
  flips only owned unread, mark-read empty-ids no-op, list returns
  empty replies array on a fresh inquiry.

- **Phase 4b scope decisions**:
  - Replies persist server-side (vs send-only) so the history
    survives the email being deleted from the inquirer's inbox.
  - Mark-as-read auto-fires on inbox view (vs explicit button),
    matching email-app conventions.
  - Threading: one direction only. Inbound replies from the
    inquirer would need an inbound-email webhook (Resend Inbound
    or a SendGrid Parse equivalent) — flagged as a follow-up.

## 2026-06-08 — T-037: cursor pagination + unmapped-artist popup + bbox/pagination fixes

Closes T-037 (cursor pagination on `/v1/search`). Bundled with the
unmapped-artist click affordance, the bbox-clipping bug for filtered
map mode, and the pagination/map-sync fix that fell out of testing.

- **T-037 cursor pagination.** New `ml_art_core::cursor::PageCursor`:
  opaque base64url-encoded JSON (`{"o": <offset>}`). Forward-
  compatible — the shape can swap for keyset (`{"k": [score, id]}`)
  later without changing the API surface, since the cursor is
  opaque to clients. `SearchParams.cursor` decoded server-side;
  malformed / out-of-range → 400. `MAX_CURSOR_OFFSET = 1000` caps
  deep-pagination attempts. `/v1/search` fetches `limit + 1` to
  detect a next page without a COUNT, drops the sentinel, encodes
  `next_cursor` from `offset + limit`. 6 unit tests on the cursor
  helper + 4 integration tests on `/v1/search` (roundtrip,
  no-cursor-when-all-fit, malformed-rejected, filter-threading).
  v1 trade-off: offset, not keyset. Hybrid search's RRF score is
  computed in SELECT, so true keyset would need an outer-SELECT
  subquery wrap; offset works fine for a ~2000-row corpus and the
  candidate-pool ceiling (200) keeps it bounded.

- **Web client + UI.** New `lib/searchClient.ts` browser-only fetch
  wrapper (mirrors `searchMapClient.ts` — public read, no Bearer
  needed, avoids dragging the server-only Clerk module into the
  client bundle). `<SearchSplitView>` holds the paginated items +
  cursor state. `<SearchSidePanel>` renders a "Load more" footer
  when `next_cursor !== null`; errors surface inline. The
  caption's "+" suffix now keys off `nextCursor !== null` ("more
  pages exist") instead of the stale `items.length >= pageLimit`
  ("first page hit the cap"). Note: grid-mode pagination (non-map)
  is out of scope for this commit; tagged as follow-up.

- **Unmapped-artist popup.** New `<UnmappedArtistPopup>` rendered
  via `useFocusArtist` when the clicked card's artist has no pin
  in the current set. Same Mapbox popup machinery as `<PinPopup>`,
  anchored at `map.getCenter()`. Carries the artwork's image +
  artist name + "View portfolio →" link. Neutral copy — no claim
  that the artist hasn't shared a location, because the absence
  of a pin can mean either "genuinely unmapped" OR "loaded on a
  later page" (see next bullet) and the client can't tell from
  where it sits. `FocusSignal` extended with `artistName` +
  `imageUrl` so the popup has what it needs without a lookup.

- **Bbox-clipping bug for filtered mode.** `page.tsx`'s server-
  side map fetch was passing `bbox` alongside `artist_ids`,
  clipping the pin list to the current viewport — so an artist
  whose venue was off-screen (e.g. Tokyo while the user panned
  to London) wasn't in the returned pins, even though they were
  in the grid result. Clicking that card fell into the unmapped
  branch incorrectly. Fix: drop `bbox` when `artist_ids` is set.
  Mirrors the existing `refetchOnPan: !hasActiveFilter` invariant
  in `useMapBboxSync` — "filter active means server returns all
  relevant pins; Mapbox decides what's visible."

- **Map sync on Load More.** When `searchClient` returns a page
  whose artists weren't in the first-page's `artist_ids` set,
  the client refetches `/v1/search/map` with the expanded set
  and replaces the map's pin state. Without this, page-2+ cards
  whose artists *do* have a location would still trigger the
  unmapped popup. Coverage check avoids the roundtrip when a
  page brings only already-covered artists. `pins` state lifted
  to `<SearchSplitView>` as a single source of truth — three
  update paths now converge there: server-prop change (filter),
  pan-refetch callback from `<SearchMap>`, and Load-more refetch.

- **Refactor.** Removed `visiblePins` mirror state in favour of
  the unified `pins`. `SearchSidePanel`'s `pageLimit` prop and
  the matching plumbing in `<SearchSplitView>` / `page.tsx` are
  gone — `hasMore` (from cursor presence) is the truthful signal.

## 2026-06-08 — T-045 L4: split-view polish (caption, mobile bottom-sheet)

Closes T-045. Two of the three planned sub-items shipped; the third
(pan-aware sort) was prototyped and pulled.

- **"N of M mapped" caption.** Replaces both the old "24+ WORKS"
  count line and the disconnect-explainer status box ("No public
  venues for these results") in one shot. Compares the loaded
  artworks against the live pin set: an item is "mapped" when its
  artist has at least one pin currently visible on the map. Reads
  as `N of M mapped` always — never `All M+ mapped` (which would
  be a contradiction: `+` means truncated, `all` implies complete).
  When `mappedCount === 0` and there are items, an inline
  "Back to Works →" link preserves the disconnect-explainer's
  affordance to escape map mode without the hostile copy.

- **Pin set lifted from SearchMap → SearchSplitView.** New
  `onPinsChanged?: (pins: MapPin[]) => void` prop on `<SearchMap>`
  fires whenever `useRefetchPins` updates its internal state (and
  also from the no-Mapbox-token fallback path via a small
  `useFallbackPinsNotifier` helper, so the caption is correct in
  restricted-network environments too). SearchSplitView holds the
  shadow copy with a prev-prop derived-state sync so a navigation
  that pushes new server-side pins lands in the same render.

- **Mobile bottom-sheet.** On `<lg` viewports the side panel
  becomes a fixed-bottom sheet with two snap states: **peek**
  (3rem handle showing the mapped count + chevron) and **expanded**
  (~70dvh, cards scrollable inside). Tap the handle to toggle.
  Map fills the viewport above so the user sees geography first
  and pulls up cards on demand. Desktop layout (sticky left
  column) is unchanged. CSS-driven via `overflow-hidden` +
  `transition-[max-height]`; no JS-driven animation.

- **Pan-aware sort: prototyped, removed.** First draft floated
  cards-with-visible-pins to the top of the sidebar on every pan.
  In practice the cards jumping mid-scroll was disorienting —
  the user expects the sidebar to stay stable, with pan only
  changing what's visible on the map. Reverted, left a comment
  in `SearchSplitView` explaining why so the next person doesn't
  reintroduce it.

- **Disconnect explainer retired** from `SearchMapBlock` — the
  caption carries the same info more honestly. Three props that
  fed it (`gridResultCount`, `gridHitLimit`, `hasActiveFilter`)
  removed from `SearchMapBlockProps` and from `page.tsx`'s
  `mapBlockProps` payload.

## 2026-06-08 — T-045 L2 + L3: split-view interactivity, city-pivot-as-filter, location-filter parity

Builds on the L1 layout shell shipped 2026-06-07. The Works tab and the
Where-to-see-them tab are now one surface where the side-panel cards
and the map pins respond to each other. Also fixes a long-standing
papercut: clicking a city chip used to just move the camera; now it's
a real `location` filter that narrows the grid + map together.

- **L2 — hover sync.** Hovering a sidebar card lifts the artist's
  pin(s) to a highlighted `feature-state` (scaled + thicker stroke).
  New `useHighlightedArtist(map, pins, slug)` hook tracks the
  previously-highlighted set so it can be cleared on the next round.
  Critical fix: added `promoteId: "location_id"` to the clustered
  GeoJSON source — without it Mapbox reassigns feature ids and
  `setFeatureState` silently no-ops on leaves.

- **L3 — click sync.** Card click flies the map to the artist's
  first pin and opens a React-DOM popup. New `useFocusArtist(map,
  pins, signal)` hook with a `{ artistSlug, tick }` signal; `tick`
  increments per click so re-clicking the same card re-fires.
  Popup is opened immediately (before `flyTo`) and anchored via
  `setLngLat`, so it tracks the pin as the camera moves — robust
  against the pin being off-screen at click time. `essential: true`
  on the fly overrides `prefers-reduced-motion`.

- **City pivot is a real filter, not a viewport hop.** Clicking
  "London" now sets `?location=London&bbox=<london>` (was: bbox
  only). Grid + map narrow together; "Showing in London ✕" FilterBar
  facet pill is the clear affordance. CityPivotStrip auto-hides
  while a location filter is selected.

- **Grid's `location` filter now also matches `artist_locations`.**
  `/v1/search` previously matched only `artists.city / .country / .location`
  (the artist's free-text "based in"). Now it ORs in an `EXISTS
  (SELECT 1 FROM artist_locations …)` clause so an artist with a
  Basingstoke studio venue appears under `?location=Basingstoke`
  even when their "based in" is blank or different. Grid + map +
  CityPivotStrip now agree on what "in X" means (all three sourced
  from `artist_locations`).

- **Camera-refit-on-clear.** New `useFitToInitialPins(map, initial,
  urlBbox)` hook plugs the "filter changed but URL didn't trigger a
  fit" gap that used to leave users zoomed to the previous city
  after a clear. Listens on `initial` *identity* (the server-pushed
  pin set), so client-side pan refetches don't yank the camera.
  Defers to `useUrlBboxFitBounds` when bbox *just* transitioned to
  a new non-empty value (chip-click case).

- **FilterBar's location clear now also drops `bbox`.** Location and
  bbox are conceptually linked (filter + its viewport hint).
  Clearing one without the other left the server-side map fetch
  spatially clipping to the old city, so `initial` came back local
  even though the grid was global. Fixed once at the source —
  `LocationPill` submit and the "Clear filters" button — which
  covers facet ×, "Clear filters", and typing a new location.

- **Component decomposition.** SearchMap.tsx is now a 100-line
  composition root pulling 8 hooks (instance, source, click handlers,
  bbox sync, url-bbox fitBounds, refetch, highlight, focus,
  fit-to-initial). Pure helpers (`bbox`, `cluster`, `geojson`,
  `url`, `constants`) live under `lib/searchMap/`. React-DOM popups
  via `Popup.setDOMContent` + `createRoot` replace the old escaped-
  HTML strings. New `<SearchSplitView>` (cross-pane state),
  `<SearchSidePanel>` + `<SidePanelCard>` (inlined click-target,
  no nested links), `<SearchMapBlock>` (chrome around the map),
  `<FilterPill>` (artist scoping pill — location is owned by
  FilterBar to avoid duplicate clears).

- **`lib/format.ts` extracted from `lib/api.ts`.** `formatPrice`
  and `formatDimensions` are pure formatters; they don't need the
  Clerk-server module that `lib/api.ts` imports. Splitting them
  out fixes the "'server-only' cannot be imported from a Client
  Component" error that turned up when `<SidePanelCard>` (client)
  needed `formatPrice`. `lib/api.ts` re-exports for backward compat.

- **Pan-loop fix.** Next 15 made `useSearchParams` reactive to
  `history.replaceState`, which caused the bbox-write handler to
  feed itself (pan → URL write → reactive bbox → fitBounds →
  moveend → URL write …). `useUrlBboxFitBounds` now guards via
  `bboxesApproxEqual` (0.01° tolerance) so our own writes resolve
  as no-ops; only true external bbox changes trigger an animation.

- **No refetch on pan when filtered.** When any text/medium/location/
  artist filter is active the server has already returned every
  matching pin (cap 500); Mapbox owns "what's visible" natively
  as the camera moves. Gated via `refetchOnPan` in
  `useMapBboxSync`. URL bbox still writes for shareability.

## 2026-06-01 — T-008b: uploads-bucket moderation

Parallels T-008 for the visual-search `uploads` bucket. Closes the
abuse vector where anonymous users could upload arbitrary images and
use them as search anchors before any moderation ran.

- **`JobEvent::UploadModerate { upload_id }`** + handler dispatch in
  `core::jobs`. New `moderate_upload` handler in `core::moderation`
  mirrors `moderate_artwork_image`.
- **`uploads::create`** enqueues with idempotency key
  `moderate:upload:{id}` after the row + S3 PUT land.
- **Visual-search anchor lookup** (`/v1/search?image_upload_id=…`)
  filters `moderation_status != 'rejected'` — rejected rows return
  404, same shape as a non-existent upload (so the abuse path can't
  tell whether the upload landed). Pending + approved still resolve
  so the uploader's own immediate search doesn't 404 during the
  upload→worker race window.
- **8 integration tests**: enqueue + payload + pending default,
  Disabled approves, canned rejects, missing-row no-op, rejected
  → 404, pending → 200, approved → 200, idempotency dedup. 1 new
  unit test on `ModerationClient`.

## 2026-06-01 — T-033: anonymous-trail merge on sign-in

Closes the "I signed up but my anonymous saves are gone" papercut.
Anything the user did keyed off the `anon_id` cookie (uploads today;
events tomorrow when T-016 writers land) gets stamped with their
now-known `user_id` once they sign in.

- **`POST /v1/me/merge-anonymous`** — auth-required, takes the calling
  user's `X-Anonymous-Id` header. Transactionally updates both
  `uploads` and `events` rows where `anonymous_id = $anon AND
  user_id IS NULL`. Returns the merged counts. Idempotent — second
  call is a no-op because the `IS NULL` predicate has nothing left
  to match. Ownership-safe: never overwrites an existing `user_id`,
  so a shared-machine anon cookie can't steal another user's rows.

- **`POST /api/me/merge-anonymous`** Next.js route handler bridges
  the client → API call (server-to-server via `apiFetch` which
  already attaches Bearer + anon-id).

- **`<AnonymousMergeBridge />`** client component mounted in the
  root layout. On sign-in, fires the bridge once per browser session
  (gated by `sessionStorage['mlart_anon_merged']`). Errors re-arm the
  marker so the next navigation retries; the underlying API is
  idempotent so re-firing is safe.

- **8 Rust integration tests** covering happy path, no-anon-header
  no-op, no-rows no-op, second-call idempotency, ownership-safety
  (Bob's rows don't flip to Alice), per-user anon isolation, 401 for
  unauthed, 400 for malformed anon header.

## 2026-06-01 — T-011 Phase 4: studio inquiries inbox

Closes the loop on T-032 — artists now have an in-app view of every
inquiry addressed to them, not just the Resend email notification.

- **`GET /v1/studio/inquiries`** — read-only list, newest first, with
  `?status=pending|delivered|all` filter. Returns artwork title +
  primary image (filtered through the T-008 `approved` guard) +
  inquirer name/email + message + budget + derived status
  (`"delivered"` when `delivered_at IS NOT NULL`, else
  `"pending_verification"`). 50-row hard cap; cursor lands with T-037.
- **`/studio/inquiries` Next.js page** — server-rendered list with
  filter-pill links (matches `/studio` toolbar style). Each card shows
  thumbnail + sender (mailto link) + relative timestamp + budget +
  full message + status badge.
- **Studio nav** gains an Inquiries → link next to Settings →.
- **9 Rust integration tests** covering ownership boundary (Bruno's
  inquiries don't leak into Alice's inbox), the three status modes,
  newest-first ordering, derived `status` string, `budget_range`
  jsonb round-trip, 401 for anonymous, 404 for non-artist users, and
  tolerance on unknown `status` query values.

**Deferred:** reply-from-inbox UX, mark-as-read, archive, and analytics
dashboards. All blocked on the events-table writes that haven't
landed yet (Phase 4b).

## 2026-06-01 — T-008: artwork-image moderation pipeline

Closes the public-surface trust gap — freshly-added images no longer
appear on artist profiles / search / collections / artwork detail
until they've been through the moderation handler. The Real
Rekognition client is deferred to when our AWS deploy lands; until
then `Disabled` auto-approves (matches today's effective behavior,
but the queue + handler + filtering plumbing is now in place).

- **`core::moderation`** — `ModerationClient` enum with `Disabled`
  (auto-approves; default in dev) and `for_tests` (canned
  `(s3_key → result)` map). No `Real` variant yet: when our AWS
  deploy lands, that's a one-spot change. `from_env()` reads
  `REKOGNITION_ENABLED`; logs a warning when it's true but the Real
  path isn't wired, then falls through to `Disabled`. 3 unit tests.

- **`JobEvent::ArtworkImageModerate { artwork_image_id }`** + handler
  dispatch in `core::jobs`. The handler (`moderate_artwork_image` in
  `core::moderation`) loads `s3_key`, calls the client, writes
  `artwork_images.moderation_status` (`approved` | `rejected`).
  Idempotent — re-runs replay the same verdict.

- **Enqueue from `studio::artworks::add_image`** with idempotency key
  `moderate:artwork_image:{id}`. Same shape as the inquiry +
  geocoding handlers.

- **Public surfaces now filter `moderation_status = 'approved'`**
  (previously `!= 'rejected'` at the one site that filtered, with no
  filter elsewhere — pending images were slipping through). Tightened
  in `artwork.rs` (detail + similar), `artist.rs`, `search.rs`,
  `search_map.rs`, `neighborhoods.rs`, `me/collections.rs`. Studio
  routes are unchanged — the artist sees their own pending +
  rejected rows.

- **`JobsDeps` extended** with `moderation: ModerationClient`;
  jobs-worker builds `ModerationClient::from_env()`. New test helpers
  in `tests/common`: `app_with_keyword_only_postgres_jobs`,
  `app_with_auth_fixed_vector_postgres_jobs`.

- **Tests**: 7 integration tests in `moderation_test.rs` covering
  enqueue, public-surface hiding, approve-via-Disabled,
  reject-via-canned, missing-row no-op, full enqueue→handle→approve
  round-trip, and idempotency dedup. 3 unit tests in `core::moderation`.

- **Env**: optional `REKOGNITION_ENABLED` (commented in `.env.example`).
  No new required vars.

**Deferred:**
- Real Rekognition wiring (`aws-sdk-rekognition` dep + IAM + region).
  Open as T-008 follow-up to land alongside the AWS deploy.
- Moderation on the `uploads` bucket (visual-search uploads). The
  table column already exists; needs a parallel `UploadModerate`
  variant + enqueue from `uploads::create`.
- Surfacing the rejection reason in studio (the `labels` field on
  `ModerationResult` is logged but not persisted).

## 2026-05-29 — T-032: real inquiry email delivery via Resend + jobs queue

Closes the biggest "demoware" gap in the inquiry flow — the
verification email + the artist notification email both now actually
send (when `RESEND_API_KEY` is set). First handler-pair to land on
the new jobs queue (T-044), validating the pattern.

- **`core::emails`** — `EmailClient` enum with `Real` (Resend HTTP),
  `Disabled` (logs + returns Ok), and `for_tests` (in-memory capture).
  Same shape as `Embedder` / `GeocodingClient` / `JobsBackend`.
  Reads `RESEND_API_KEY` + `RESEND_FROM_EMAIL`; falls back to
  `Disabled` when either is unset (local dev with no paid key).
  6 unit tests on disabled-noop / test-capture / template rendering.

- **Two email templates** (`core::emails::templates`):
  - `verification(verify_url, name, artwork_title, artist)` — the
    confirm-your-email link for anonymous inquirers.
  - `delivered_to_artist(artwork_url, title, image, name, email,
    message, budget)` — the actual "you have an inquiry" email.
    Hand-escaped HTML; user input goes through `escape_html`.
    Newlines in the message become `<br />`. Empty budget +
    missing image both gracefully omit.

- **Two `JobEvent` variants** + handler dispatch in `core::jobs`:
  - `InquirySendVerification { inquiry_id }`
  - `InquiryDeliverToArtist { inquiry_id }`
  - Handlers load the inquiry + artwork + artist + (artist user)
    rows via a single join, render the template, call
    `EmailClient::send`. Reply-to is set to the inquirer's email
    so the artist can just hit reply.

- **`JobsDeps` extended** with `emails: EmailClient` + `web_base_url:
  String`. main.rs builds `EmailClient::from_env()`; jobs-worker
  picks it up; test helpers use `for_tests()`.

- **`Config`** — new `web_base_url` (defaults to
  `http://localhost:3000`). Read from `WEB_BASE_URL` env var.

- **`inquiries.rs` enqueue sites** — three call sites, one variant
  each:
  - Anonymous create → `InquirySendVerification`
    (replaces the old `TODO(T-032)` comment)
  - Signed-in create → `InquiryDeliverToArtist`
    (bypasses verification — Clerk-verified email)
  - Verify endpoint → `InquiryDeliverToArtist`
    (fires after the anonymous flow flips `delivered_at`)
  - All three use idempotency keys (`inquiry_verify:<id>` /
    `inquiry_deliver:<id>`) so duplicate clicks / double-fires
    don't send the email twice. Verified by the
    `idempotency_dedups_double_verify` test.

- **Tests**
  - 4 integration tests in `tests/inquiry_emails_test.rs` —
    signed-in path enqueues only deliver; anonymous path enqueues
    only verification; verify endpoint adds the deliver job; double-
    verify dedups. All use `app_with_postgres_jobs` (new test
    helper) so the rows land in the actual `jobs` table for SQL
    assertions.
  - 6 new unit tests in `core::emails`.
  - 226 Rust total (was 216; +10).

- **Env additions** in `api/.env.example` + `api/.env`:
  - `RESEND_API_KEY` (optional; absent → disabled)
  - `RESEND_FROM_EMAIL` (required for the real path)
  - `WEB_BASE_URL` (defaults to `http://localhost:3000`)

How it actually runs locally:
- Without `RESEND_API_KEY`: every email path logs at info, returns
  Ok. The jobs row still completes; the artist just never receives
  an actual email. Same degrade-gracefully shape as Mapbox / Jina.
- With `RESEND_API_KEY` + a verified domain: real emails sent via
  Resend. Free tier is 3k/mo + 100/day.

What's deferred:
- T-008 (Rekognition moderation) — same pattern, one new
  `JobEvent::ImageModerate` variant + handler. Probably the next
  thing to pick up.
- The web `/inquiries/verify/[token]` page already exists from
  T-001; this work doesn't touch it.

---

## 2026-05-29 — Jobs queue (T-044): Postgres local, SQS+Lambda prod

Foundation for every future background job. Closes the worker-runtime
question raised in the state-of-the-build review. See `decisions.md`
2026-05-29 — jobs queue.

- **Migration `0012_jobs.sql`** — `jobs` table with `kind`, `payload`,
  `status` enum (pending|running|done|failed), `attempts`,
  `max_attempts`, `next_run_at` (exponential backoff), unique
  `idempotency_key`, `last_error`. Partial index on
  `(next_run_at)` where `status = 'pending'` keeps the worker scan
  constant-cost as `done` rows accumulate.

- **`core::jobs`** — driver-agnostic abstraction:
  - `JobEvent` tagged enum (`#[serde(tag = "kind", content = "payload")]`).
    First variant: `ArtistLocationGeocode { location_id }`.
  - `JobsBackend` enum with `Postgres` and `for_tests` variants.
    Same shape as `ObjectStore` / `GeocodingClient`.
  - `enqueue(event, opts)` — opts carry `idempotency_key` +
    `max_attempts`. `ON CONFLICT DO NOTHING` on the key makes
    duplicate enqueues a silent no-op.
  - `postgres::claim_one` — single-statement `UPDATE … RETURNING`
    over a `FOR UPDATE SKIP LOCKED` subquery so multiple workers
    can run concurrently. Returns the post-increment `attempts`
    so callers can correctly decide retry vs fail.
  - `postgres::mark_done` / `mark_failed_or_retry` — terminal vs
    backoff (2 → 8 → 32 → 128 … capped at 1h).
  - `JobsDeps` — minimal struct carrying what handlers need (pool +
    geocoder for now). Analogous to AppState but stripped to the
    worker's surface.
  - `handle(event, deps)` — dispatch fn. New job = new match arm
    here + new domain-module handler. The driver doesn't grow.

- **`api/crates/jobs-worker`** — new crate, one binary. Polls every
  2s; on claim, decodes the event, calls `core::jobs::handle()`,
  marks done / retry / failed. ~100 lines.

- **Canary on geocoding** — replaced `core::geocoding::trigger_background_geocode`
  (the only `tokio::spawn` in the codebase) with `state.jobs.enqueue()`.
  Both studio CRUD call sites (POST + PATCH on `/v1/studio/locations`)
  now go through the queue. The handler `geocode_and_update` is
  unchanged — just driven differently.

- **`AppState` swap**: dropped the `geocoder: GeocodingClient` field
  (nothing in api-search read it after the canary) and added
  `jobs: JobsBackend`. main.rs builds `JobsBackend::postgres(pool)`;
  test helpers use `JobsBackend::for_tests()` (in-memory capture).

- **`make dev` integration** — `scripts/dev.sh` spawns `jobs-worker`
  alongside the api under cargo-watch. Handler edits + migration
  changes both trigger restarts. Logs go to `/tmp/worker.log`.

- **Tests**
  - 4 unit tests in `core::jobs` (enum serialization, kind dispatch,
    in-memory backend capture, decode round-trip)
  - 6 integration tests in `tests/jobs_test.rs` (enqueue, idempotency
    dedup, claim+running, full enqueue→handle→done loop, retry+terminate
    via backoff, studio-create enqueue side-effect)
  - 216 Rust total (was 206; +10)

- **Smoke-tested end-to-end**: enqueued a real `artist_location_geocode`
  job via psql, observed the local worker pick it up, run the
  Mapbox-disabled handler, and mark `done` within the poll interval.

What's not built yet:
- The SQS+Lambda driver (`JobsBackend::Sqs` + `jobs-lambda` crate).
  Ships when we deploy to AWS — same handler code runs unchanged.
- Worker observability beyond `tracing` logs. CloudWatch via
  `tracing-cloudwatch` lands with the prod driver.

Next: T-032 (Resend email delivery) + T-008 (Rekognition moderation)
become each "add a `JobEvent` variant + handler" — single PRs that
ride on top of this foundation.

---

## 2026-05-29 — Artist UX polish: T-039 price input + T-040 location feedback

Two real-artist friction points from the demo session.

- **T-039 — currency-aware price input**
  - `lib/parsePrice.ts` accepts what humans type: `£120`, `$1,200`,
    `EUR 4500`, `120.50`, `1.234,50` (European decimal). Strips
    currency symbols + thousands separators; parses decimals as the
    currency's minor-unit places (USD/GBP/EUR = 2, JPY = 0)
  - Inverse `formatPriceForInput` pretty-prints back into the input
    on blur so artists see `120.00` not `12000`
  - ISO 4217 minor-unit table for ~20 common currencies; falls back
    to 2 places for unknown codes (parser still accepts them inline,
    e.g. `AUD 200`)
  - `ArtworkEditModal` price field is now `<input type="text">` with
    `inputMode="decimal"` + a currency `<select>` next to it (USD,
    GBP, EUR, CAD, AUD, JPY, CHF, SEK, NOK, DKK). Validation runs on
    submit; errors surface inline next to the field. On-blur
    re-formats to canonical "120.00" shape and updates the selected
    currency if the artist typed a symbol or code inline
  - Removed the redundant "Currency" text-input field — the dropdown
    next to the price input replaces it
  - 17 new vitest tests on `parsePrice` + `formatPriceForInput` +
    `minorUnitsFor`

- **T-040 — geocode feedback in studio locations**
  - `StudioLocationsManager`'s pin status is now three-state:
    - `geocoded_at` null → "Locating…" (amber, in-flight)
    - `geocoded_at` set + lat null → "Couldn't find this address —
      try adding city + country" (red, actionable error)
    - `geocoded_at` set + lat set → "Pin set · {city}, {country}"
      (muted, success)
  - Previously the failure case looked identical to "Locating…"
    forever, leaving the artist with no signal that they needed to
    edit. The Mapbox response is on the row already; this is pure
    UI surfacing

- **Tests + checks**
  - 44 vitest tests (was 27, +17 from parsePrice)
  - 206 Rust (unchanged — both fixes are web-side)
  - fmt + clippy + ESLint + tsc all clean

---

## 2026-05-29 — Map discovery v1 (T-041 + T-042 + T-043)

Closes the two user journeys we identified for the map: "find galleries
near me" and "see where an artist's work is." Three small slices on top
of T-038, no new infra.

- **T-041 — artist filter + "See on map" CTA**
  - `GET /v1/search/map?artist=<slug>` — pins down to that artist's
    venues only. Composes with `q` / `medium` / `bbox` / `location`
    (e.g. `?artist=alice&bbox=12,52,14,53` for "Alice's venues in this
    area"). Uses the existing `artists_slug_idx` index — cost is
    constant
  - "See on full map →" link in the `ArtistLocationsMap` header on
    `/artists/[slug]` (when locations > 0)
  - Scoping pill on `/search?map=1` when artist is active: "Showing
    where to see **Alice Test** [Clear filter]". Clear preserves
    every other URL param
  - 3 new integration tests (happy / unknown-slug / bbox composition)

- **T-042 — city-pivot pills**
  - `GET /v1/search/map/cities` — top-N cities by venue count, each
    with `count`, centroid, and tight bbox of all pins in that city.
    Single GROUP BY query; honors `?limit=` (default 12, max 100).
    No rate-limit layer (light static read)
  - Excludes pre-geocode rows + inactive artists. Ordered by
    `count DESC, city ASC` so ties are stable
  - New `CityPivotStrip` client component — horizontal scrollable
    pill row above the map ("London (12) · Berlin (8) · …"). Each
    pill links to `/search?map=1&bbox=<padded-city-bbox>` so the
    URL stays bookmarkable. Degenerate single-pin bboxes get padded
    to ~5km half-extent so we don't zoom to street level
  - Solves the cold-start "blank world" problem — the strip
    surfaces where there's *anything* to see
  - 5 new integration tests (counts / pre-geocode exclusion /
    ordering / limit cap / multi-pin bbox)

- **T-043 — "Near me" geolocation**
  - New `NearMeButton` component using `navigator.geolocation`. Two
    variants: `inline` (sits next to the Grid/Map toggle on
    `/search`) and `hero` (homepage hero affordance under the search
    bar)
  - Self-hides when geolocation isn't available (SSR, opted-out
    browser, embedded webview) — no "this won't work" surface
  - PERMISSION_DENIED gets a soft inline message; other errors are
    reported via `reportError`. 10s timeout for cold GPS warmup
  - Sets a ~5km half-extent bbox around the user's coords and
    navigates to `/search?map=1&bbox=…` — the existing `SearchMap`
    component handles the fitBounds on mount

- **Homepage hero gets a map row**
  - Below the existing search bar + camera icon: "📍 Near me · or ·
    Explore the map →" — the first user-visible entry point into
    geographic discovery without going through `/search`

- **Tests + checks**
  - **Rust: 206/206** (was 201; +5 cities tests)
  - Vitest: 27 (unchanged)
  - fmt + clippy + ESLint + tsc all clean; `pnpm build` clean

End-to-end demo path:
1. Land on `/` — hero search + "📍 Near me · or · Explore the map →"
2. Click "Explore the map →" → `/search?map=1` → see city pills
   ("London (1) · Basingstoke (1)") even at world zoom
3. Click "London" → map zooms to London bbox → see the pin
4. Click pin → popover with artist preview + "View portfolio →"
5. From `/artists/josh-matthews` → "See on full map →" → map filtered
   to only Josh's venues with the scoping pill

Out of v1 (still deferred):
- Server-side sort by distance from a point (the bbox refetch already
  drops out-of-bounds pins implicitly)
- Mapbox Places autocomplete on the city pivot (the pill strip
  covers 80% of intent)
- IP-based geo for first-page-load default zoom
- Saved-artists → "where are they showing" digest (needs follow signal)

---


## 2026-05-28 — Onboarding Phase 1 (T-012): self-serve artist mint

Closes the biggest "demo-only" gap in v1: any signed-in user can now
mint their own `artists` row, fill in a profile, add work, list venues,
and publish — no admin intervention needed. AI-assisted pieces (website
scrape, LLM artwork metadata) stay deferred until Inngest lands;
shipped path is intentionally lean.

- **API** (`api-search::onboarding`)
  - `POST /v1/onboarding/start` — body `{display_name, location?}`.
    Validates (1–100 char name, ≤200 char location), checks the caller
    doesn't already have an `artists` row, generates a slug via
    `slugify()` + collision-suffix loop (`jane-doe`, `jane-doe-2`, …),
    inserts the row with `status='pending'` + a platform-inbox default
    for `inquiry_preferences`, and flips `users.is_artist=true`. Returns
    201 + the new `StudioArtist`
  - `POST /v1/onboarding/complete` — flips `status: pending → active`
    in a CASE-guarded UPDATE so a re-submit on an already-active artist
    returns the unchanged row (200, idempotent)
  - `slugify()` is a pure function with 6 unit tests (whitespace +
    punctuation collapsing, leading/trailing dashes, lowercasing,
    non-ASCII fallback, empty-input fallback to "artist")
  - 10 integration tests in `tests/onboarding_test.rs` — mint /
    already-an-artist / empty-name / overlong-name / slug collision /
    complete-flip / complete-idempotent / complete-no-artist / auth
    boundaries
  - `ApiError::Conflict` not added (would touch every other handler);
    "already an artist" returns 400 with a clear detail string. Comment
    inline notes the sharpening path if a real call-site needs 409

- **Wizard** (`/onboarding`)
  - Server-rendered orchestrator at `app/onboarding/page.tsx`. Reads
    the user's current artist state (`getStudioMe`), gates each step:
    no-artist → forced to `?step=identity`; already-onboarded who
    lands on identity → bumped to `?step=profile`; active artist on
    review → "View your profile →" instead of "Publish"
  - Five step components under `components/onboarding/`:
    - **IdentityStep** — `display_name` + optional `location`; submits
      to `startOnboarding` server action
    - **ProfileStep** — bio, statement, website; wraps
      `updateStudioSettings`; skippable
    - **ArtworksStep** — reuses `ArtworkEditModal` (T-011) for create
      + edit; skip-friendly (zero artworks allowed)
    - **LocationsStep** — reuses `StudioLocationsManager` (T-038 G3)
    - **ReviewStep** — summary + Publish, calls `completeOnboarding`,
      redirects to `/artists/<slug>`
  - **StepNav** — chip strip with numbered steps. Past + current
    chips are clickable; future steps are muted (no jump-ahead).
    Server-rendered, no interactivity — current step is a prop

- **Cross-cutting**
  - `/studio` redirects signed-in non-artists to `/onboarding`
    (previously: stale "by direct invitation only" empty state)
  - `/studio/settings` same redirect
  - `TopNav` gains a "Studio" link for signed-in users (alongside
    Collections). `/studio` handles the artist / non-artist branch via
    its own redirect, so a single link covers both UX paths — no
    per-render `getStudioMe()` call

- **E2E**
  - New `e2e/tests/21-onboarding-signed-in.spec.ts` — drives the full
    wizard: visit `/studio` → bounce to `/onboarding` → fill identity
    (unique-per-worker display name) → skip profile / artworks /
    locations → publish → land on `/artists/<slug>` with the
    display-name heading
  - Updated specs 17 + 18 to assert the redirect-to-onboarding shape
    instead of the now-removed empty states

- **Server actions** (`app/actions/onboarding.ts`)
  - `startOnboarding(body)` + `completeOnboarding()` — same pattern as
    `actions/studio.ts`: Bearer never touches the browser,
    `revalidatePath` on the affected slug-keyed pages

- **Tests + checks**
  - Rust: **198/198** passing (+16 from onboarding)
  - Vitest: 20/20
  - fmt + clippy + ESLint + tsc + production build clean

**Deferred to T-012 Phase 2** (when Inngest lands):
- `POST /v1/onboarding/scrape` — website pre-fill from a portfolio URL
- `POST /v1/onboarding/extract` — Anthropic-assisted artwork metadata
  extraction from free-text descriptions

Next priorities in `TODO.md`: Inngest runtime (unblocks T-032 email,
T-008 moderation, T-012 Phase 2, and the T-038 geocode swap as a
side-effect), then T-033 anon→user merge, then UI polish.

---

## 2026-05-28 — Geography slice G6: E2E + docs sweep, T-038 closed

Wraps up T-038. Geography is now end-to-end shippable: schema, geocoder,
studio CRUD + UI, artist-profile map, search map mode. All five subtasks
in `TODO.md` flipped to ✅ with a small follow-up list (real Inngest
swap, seeded demo locations, geographic neighborhoods) carried forward.

- **E2E** — `e2e/tests/20-geography-search-map.spec.ts`
  - Grid / Map view toggle renders on `/search` with the correct
    `aria-selected` default (Grid)
  - Clicking "Map" navigates to `?map=1` and the toggle reflects
    selection
  - Toggling back to Grid preserves filters (`q=ukiyo` survives) but
    drops the `map` and `bbox` params (no stale-bbox poisoning)
  - Map region (token set) or fallback empty-state (token absent) is
    present without runtime errors — same `.or()` pattern as other
    E2Es that tolerate the missing-key path
  - Direct API hit asserts malformed `bbox` returns 400 via Playwright's
    `request` fixture
  - Full pin-clicking / cluster-zoom path isn't asserted in CI — the
    seeded WikiArt corpus has no `artist_locations` rows, so a real
    end-to-end demo needs a manual seed step. Tracked as a follow-up
    in `TODO.md`

- **Docs**
  - `03-api-data-spec.md` — added `/v1/studio/locations` CRUD,
    `/v1/search/map`, and the full `artist_locations` table definition
    with indexes + trust-model note
  - `99-deferred.md` — annotated "Geographic discovery / Phase 1" with
    a 2026-05-28 update; map view marked shipped, geographic
    neighborhoods + "based in" filter still deferred
  - `TODO.md` — `T-038` struck through with the five sub-phase
    summaries inline + a small follow-up list (Inngest swap, seeded
    demo locations, geographic neighborhoods)

- **Final test tally:** 182 Rust + 20 vitest + ~24 Playwright specs.
  fmt + clippy + eslint + typecheck + production build all clean.

T-038 closed. The biggest unbuilt v1 piece is now `T-012` (onboarding
flow) and `T-032` (real email delivery via Resend + Inngest). See
`TODO.md` for the active queue.

---

## 2026-05-28 — Geography slice G5: `/search?map=1` map mode (T-038)

Closes the user-facing arc of T-038. Viewers can now toggle the search
page between the grid of artworks (existing behavior) and a Mapbox GL
JS map of venues, pan/zoom to explore, and click pins to jump to artist
profiles. Filters from the URL (`q`, `medium`, `location`) compose with
map mode out of the box.

- **`GET /v1/search/map`** (`api-search::search_map`)
  - Returns a flat list of `MapPin` rows — one per geocoded
    `artist_locations` row matching the active filters
  - Filters: `q` (artwork tsvector via `plainto_tsquery('english', …)`,
    EXISTS subquery to keep the join one-per-location), `medium`
    (same EXISTS shape), `location` (case-insensitive `LIKE` on
    `al.city`, with user wildcards escaped), `bbox` ("west,south,east,
    north" — Mapbox's `bounds.toArray().flat()` ordering)
  - Hard cap of 500 pins per response (Mapbox clusters smoothly into
    the low thousands; gives us room to add server-side aggregation
    later)
  - Bbox validator rejects malformed inputs (wrong arity, non-numeric,
    out-of-range lat/lng, inverted) with RFC 7807 400
  - SQL composed string-piece-by-string-piece with positional bind
    markers — same pattern as `/v1/search`. `AssertSqlSafe` is the
    audited escape from sqlx's `'static str` bound: no user input ever
    lands in the SQL string itself
  - Why a separate endpoint, not a `?map=1` flag on `/v1/search`:
    `/v1/search` ranks **artworks** via the hybrid keyword + vector
    fusion. Map mode wants **venues**, and "rank by relevance" doesn't
    translate. Two narrow endpoints beat one with two divergent
    codepaths

- **`web/src/components/SearchMap.tsx`** — client component
  - Three render paths (same model as `ArtistLocationsMap`): no
    `NEXT_PUBLIC_MAPBOX_TOKEN` → non-interactive grid of pin cards;
    token present → Mapbox GL JS map; init error → fallback list with
    error note
  - GeoJSON source with `cluster: true` — clustering is entirely
    client-side and free; pin count up to 500 per response stays
    smooth
  - Cluster click → `getClusterExpansionZoom` + `easeTo`; pin click
    → popup with thumbnail + artist name + "View portfolio →" link
  - Pan/zoom → `bbox` query param updated via `history.replaceState`
    (so the URL stays shareable but doesn't push a history entry per
    move) → refetches `/v1/search/map?bbox=…` via the new
    `searchMapClient` helper
  - GL JS bundle dynamically imported inside the effect so it
    doesn't ship in the initial JS bundle for users who never toggle
    to map mode

- **`web/src/lib/searchMapClient.ts`** — small browser-only fetch
  wrapper for `/v1/search/map`. Lives in its own module so the
  `SearchMap` client component doesn't drag in `lib/api.ts`'s
  `apiFetch` (which dynamic-imports `@clerk/nextjs/server` and pollutes
  the client bundle). The endpoint is public — no Bearer needed — so a
  plain `window.fetch` is correct
  - Pattern matches `decisions.md` 2026-05-27 client/server import
    boundary: anything the client touches must not transit
    `@clerk/nextjs/server`

- **`/search` page** — Grid/Map toggle tab strip below the FilterBar.
  Tabs preserve all other query params; only `map` flips. `bbox` is
  dropped when leaving map mode so a stale bbox doesn't poison the
  next grid query
  - Server-side initial fetch on `map=1` so the first render has data
    before the client Mapbox module loads; client takes over from
    there

- **`lib/api.ts`** — new `MapPin`, `MapSearchParams`, `searchMap`
  (server-side variant; only used by the server-rendered initial
  fetch). The client variant lives in `searchMapClient.ts`

- **Tests**
  - 8 unit tests on bbox parsing + location filter escaping
    (`api-search::search_map::tests`)
  - 14 integration tests in `tests/search_map_test.rs` — happy path,
    bbox London/Berlin/Atlantic/global, malformed/inverted/out-of-
    range bbox 400s, location substring filter, medium per-artist
    composition (Painting → Alice only, Sculpture → Bruno only,
    Holography → empty), `q` tsvector match (cobalt → Alice), `q`
    no-match, pre-geocode rows never leak
  - 182 Rust tests passing (was 160 — +22 from G5)
  - 20 vitest unchanged; `pnpm build` clean (no client/server bundling
    regression)

Next up: G6 — E2E coverage, doc updates, CHANGELOG sweep. Then T-038
itself is done.

---


## 2026-05-28 — Geography slice G4: artist-profile map widget (T-038)

First user-visible map on the platform. `/artists/[slug]` now renders a
"Where to see this work" section with an interactive Mapbox GL JS map
pinned to the artist's `artist_locations` rows (geocoded only — the API
filters out pending ones, so the surface stays clean).

- **`web/src/components/ArtistLocationsMap.tsx`** — client component
  - Three render paths, picked at runtime: zero locations → returns
    `null` (no section); no `NEXT_PUBLIC_MAPBOX_TOKEN` → non-interactive
    list of pin cards (same data, no JS — keeps local dev usable
    without the paid key); token present → real GL JS map
  - Dynamic `import("mapbox-gl")` inside the effect so the ~250KB
    bundle never ships for artist pages that have zero locations (most
    of them today)
  - Single pin: center+zoom; multiple pins: `fitBounds` with padding
    so they all show
  - Click → popover with name, address, optional website link, and a
    "Listed by the artist" disclosure (the v1 trust model). Popup HTML
    assembled by hand because Mapbox takes strings, not React; every
    field goes through `escapeHtml` / `escapeAttr`
  - Map errors fall back to the same list view, with a small note —
    "Couldn't load the map. Showing the list instead."
  - Style: `mapbox://styles/mapbox/light-v11`. Tilt + rotate disabled
    (no value on a profile-card-sized embed); standard nav control
    (no compass)

- **`web/src/lib/api.ts`** — `ArtistDetail` gains
  `locations: PublicArtistLocation[]`. Lighter than `StudioLocation`:
  `lat` / `lng` are guaranteed non-null on this surface because the API
  filters pre-geocode rows out of the public payload

- **`/artists/[slug]/page.tsx`** — renders `ArtistLocationsMap` between
  the header and the artist statement. Component returns `null` for
  artists with no locations, so the section silently disappears for
  the corpus that doesn't have any yet (every seeded WikiArt artist)

- **Deps**
  - Added `mapbox-gl ^3.24.0`. Bundles its own TypeScript types in v3
    (the legacy `@types/mapbox-gl` is deprecated; we briefly installed
    then removed it)
  - No new dev deps

- **Env**
  - `NEXT_PUBLIC_MAPBOX_TOKEN` documented in `web/.env.example` with
    the same "degrades gracefully when absent" framing as the other
    paid keys. Free tier covers 50k map loads/month — well above any
    v0 traffic

- **Build / lint**
  - `pnpm build` clean — the dynamic import keeps Mapbox out of the
    server bundle, no edge / RSC warnings
  - 20 vitest unchanged

Next up: G5 — `/search?map=1` toggle (grid ↔ map), with clustering and
URL-synced bounds.

---

## 2026-05-28 — Geography slice G3: studio locations CRUD + UI (T-038)

Closes the loop on G1 (schema) + G2 (geocoder): artists can now self-list
the galleries and studios where their work can be seen, and Mapbox fires
in the background to pin them. Public artist pages don't render the map
yet (G4) but the JSON they return already includes geocoded rows.

- **`POST/GET/PATCH/DELETE /v1/studio/locations`** (`api-search::studio::locations`)
  - Resolves `current_artist_id` from `AuthedUser`; every SQL gate is
    `artist_id = $current_artist_id` so cross-artist access returns 404
    (not 403, to avoid leaking existence — same pattern as artworks)
  - `kind` constrained to `'gallery' | 'studio'`; soft per-artist cap
    of 50 rows (well above any real artist's count, hard-fails before
    the studio UI gets noisy)
  - POST inserts the row, then fires `trigger_background_geocode` so
    the HTTP response returns immediately. The studio UI reads the
    un-geocoded row and shows "Locating…"
  - PATCH clears lat/lng/city/country/geocoded_at when `address`
    changes and re-fires the geocode (mirrors the existing `location`-
    clearing pattern in studio settings)
  - DELETE soft-deletes via `deleted_at`; row stays in the table but
    drops out of every read path
  - Custom serde helper `deserialize_double_option` gives real PATCH
    semantics on `website_url` — missing key → leave alone, `null` →
    clear, string → set. Without it, plain `Option<Option<T>>` +
    `#[serde(default)]` collapses both `null` and missing into outer
    `None`, making it impossible to NULL a column over PATCH. Generic
    so future endpoints can lift it

- **`web/src/components/StudioLocationsManager.tsx`** — client component
  - Inline "Add location" form (kind / name / address / website)
  - One row per location with read + inline edit; "Pin set" or
    "Locating…" badge driven by `lat != null`
  - Delete with two-step confirm
  - **Source-of-truth model:** `initial` prop is the latest server
    snapshot; we don't mirror in local state. After every mutation we
    `router.refresh()`, which re-renders with new props. Avoids the
    `set-state-in-effect` lint trap and keeps a single source of truth
  - Polling: when any row is pre-geocode, refresh every 3s until all
    rows have pins. Stops cleanly the moment geocoding completes

- **`web/src/lib/api.ts`** — new `StudioLocation`, `CreateLocationBody`,
  `PatchLocationBody` types + `listStudioLocations` /
  `createStudioLocation` / `patchStudioLocation` / `deleteStudioLocation`
  client functions (server-only — Bearer never touches the browser)

- **`web/src/app/actions/studio.ts`** — server-action wrappers
  (`loadStudioLocations`, `createLocation`, `patchLocation`,
  `deleteLocation`) that revalidate `/studio/settings` so the next
  refresh picks up the new state

- **`/studio/settings` page** — renders `StudioLocationsManager` below
  the existing `StudioSettingsForm`. Loading failures collapse to an
  empty list (same null-collapses-failure pattern as the rest of the
  studio surface)

- **Tests**
  - 16 new integration tests in `tests/studio_locations_test.rs`:
    list / create / patch / delete happy paths + 401 / 404 / 400
    branches + ownership boundary (alice can't reach bruno's row) +
    PATCH-can-null-website-url + PATCH-address-clears-geocode +
    create-rejects-bare-url / -unknown-kind / -empty-name
  - 176 total tests passing (Rust 160 + web vitest 20 — vitest
    unchanged, all 16 new were Rust integration)

Next up: G4 — Mapbox GL JS map widget on `/artists/[slug]`, rendering
the geocoded pins this slice now produces.

---

## 2026-05-28 — Geography slice G2: Mapbox forward-geocoding (T-038)

Wires the geocoder behind a `GeocodingClient` abstraction with the same
degrades-gracefully pattern as `Embedder` and `ObjectStore`. No studio
endpoint calls it yet (G3) — G2 is the library + the integration tests
that prove the round trip works.

- **`core::geocoding` module**
  - `GeocodingClient` enum-backed sum type: `Real` (Mapbox v6 HTTP),
    `Disabled` (token absent → `Ok(None)`), `Test` (canned address →
    result map for integration tests; no network)
  - `from_env()` reads `MAPBOX_TOKEN`; empty or unset falls back to
    `Disabled`. Production code never branches on the variant directly
  - `geocode_address(addr)` → `Result<Option<Geocoded>, GeocodeError>`.
    `Some(g)` = pin lands. `None` = stamp `geocoded_at` and move on
    (covers both "Mapbox returned zero features" and "token disabled"
    so callers handle them the same way). `Err` = transient failure,
    don't stamp, the partial-index re-queues the row
  - Mapbox v6 response parsing pinned in unit tests: `coordinates`
    array, `properties.context.place.name`, `properties.context.country
    .country_code` (ISO 3166-1 alpha-2, uppercased on the way out)
- **`geocode_and_update(client, pool, location_id)`** — synchronous
  helper that reads the address, calls the client, writes lat / lng /
  city / country / geocoded_at. Public so studio CRUD can call it
  directly (G3) and integration tests can drive the path without racing
  `tokio::spawn`
- **`trigger_background_geocode(client, pool, location_id)`** — fire-
  and-forget wrapper: same logic, in `tokio::spawn`, errors logged via
  `tracing::warn`. The studio handlers will use this on POST/PATCH so
  the HTTP response returns immediately; crashes lose the in-flight task
  but `artist_locations_geocode_pending_idx` finds the row next pass.
  When Inngest lands we replace this with an `artist_location.geocode`
  function — same signature, same semantics

- **`AppState` plumbing**
  - New `geocoder: GeocodingClient` field on `AppState`; `main.rs`
    wires `GeocodingClient::from_env()`; test helpers default to
    `Disabled`
  - All 5 `AppState { ... }` constructions in `tests/common/mod.rs`
    updated via `replace_all`

- **Tests**
  - 5 unit tests in `core::geocoding::tests` covering the v6 response
    shape (happy, missing-context, empty-features) + `Disabled` /
    `Test` client behavior
  - 5 integration tests in `tests/geocoding_test.rs` exercising
    `geocode_and_update` against a real Postgres + canned client:
    writes lat / lng / city / country, stamps `geocoded_at` even when
    the client returns `None`, overwrites previous coords on re-run,
    and is a soft no-op for missing rows. 144 total tests passing (was
    134)

- **`api/.env.example`** — expanded the `MAPBOX_TOKEN` comment to call
  out the degrades-gracefully behavior and link to Mapbox's free-tier
  signup. No new keys

Next up: G3 — `/v1/studio/locations` CRUD calling `trigger_background_geocode`,
plus the studio settings UI for adding gallery / studio rows.

---

## 2026-05-28 — Geography slice G1: `artist_locations` schema + artist-detail payload (T-038)

First step of promoting geography from post-v1 to v1 (`decisions.md` 2026-05-28).
Pure data-layer change — no UI, no Mapbox call yet. Sets up the row shape that
the next sub-tasks (G2 geocoding, G3 studio CRUD + UI, G4 profile map, G5 search
map mode) build on.

- **`db/migrations/0011_artist_locations.sql`**
  - New table: `(id, artist_id, kind, name, address, city, country, lat, lng,
    website_url, display_order, geocoded_at, created_at, updated_at, deleted_at)`
  - `kind CHECK IN ('gallery', 'studio')` — shows/events deferred to post-v1
  - Address captured raw at insert; geocoder populates lat/lng/city/country async
  - Indexes: `(artist_id, display_order)` for studio + artist-profile reads;
    `(lat, lng)` partial for bbox queries on `/search?map=1`; `(city)` partial
    for the existing location-text filter; `(created_at)` partial on
    `geocoded_at IS NULL` for the geocode-job worklist
  - `ON DELETE CASCADE` on `artist_id` — locations don't outlive their artist

- **`ml_art_core::models`**
  - New `ArtistLocation` DTO; `ArtistDetail` gains `locations: Vec<ArtistLocation>`
    (serde-default so older clients can ignore it)

- **`api-search::artist`**
  - `GET /v1/artists/:slug` joins on `artist_locations`, returning only rows
    where lat/lng are non-null (geocode landed) and `deleted_at IS NULL`
  - Hidden rows: pre-geocode rows (lat/lng NULL) are gated out of the public
    payload. The studio UI will use a separate endpoint that includes them so
    the artist can see "Locating…" feedback (G3)
  - Sort: `display_order ASC, created_at ASC` — predictable order even when
    the artist hasn't reordered

- **Tests** (`api-search/tests/`)
  - Fixture: alice gets 2 location rows (1 geocoded gallery, 1 pre-geocode
    studio); bruno gets 1 geocoded; carmen gets 0
  - `artist_detail_returns_geocoded_locations` — public payload shows the
    geocoded gallery, hides the pre-geocode studio
  - `artist_detail_empty_locations_for_artist_without_any` — empty list, no
    error, when an artist has none
  - 134 Rust tests passing (was 132)

Next up: G2 — Mapbox HTTP client in `core::geocoding` + the
`artist_location.geocode` Inngest job, with no-op behavior when `MAPBOX_TOKEN`
is absent.

---

## 2026-05-28 — Visual-search UI: camera + modifier pills (T-010 Phase D)

Wires the spike-validated machinery into a real user-facing surface.
With Phase D shipped the whole `T-010` epic is done.

- **`VisualSearchUpload` (client component)**
  - Camera-icon button next to the hero search bar. Two sizes (`hero`,
    `nav`); only `hero` rendered on the homepage today, keeping the
    nav uncluttered
  - File-picker triggered via hidden `<input type="file">`. Client-side
    pre-checks: MIME type must start with `image/`; size ≤ 10MB.
    Surfaces an instant error message instead of round-tripping a 400
  - Submits the form to a server action so the file bytes never touch
    the browser's fetch layer (same Bearer + anon-id forwarding as the
    other server-action paths)

- **Server action `uploadAndStartVisualSearch`**
  - Reads the `FormData`, marshals to `uploadImageForSearch(file)` in
    `lib/api.ts`, redirects to `/search?image_upload_id=<id>` on success
  - On failure, redirects to `/search?upload_error=<msg>` so the search
    page has a place to render the friendly state without a separate
    error route

- **`ModifierBar` (client component)**
  - URL-driven pills, one per registered modifier. Toggle adds or
    removes the name from `?modifiers=`; multiple modifiers comma-
    separated. Same shape as `FilterBar`'s pills (`aria-pressed`,
    toolbar role)
  - Only renders when the URL carries `image_upload_id` — modifiers
    without an anchor are a server-side 400 (Phase C), and we don't
    want to draw a button that always fails

- **`/search` page integration**
  - New `Search` type fields: `image_upload_id`, `modifiers`,
    `upload_error`
  - Visual mode (`image_upload_id` present): renders an upload-error
    banner if applicable + a `VisualAnchor` strip (truncated id +
    "Clear image" link) + `ModifierBar` + the existing `FilterBar`
    above the results grid. `describeQuery` mentions the image and
    modifiers in the human-readable summary
  - `listSearchModifiers()` is called only in visual mode — saves the
    extra round-trip on every plain-text search

- **`lib/api.ts` additions**
  - `SearchParams.image_upload_id`, `SearchParams.modifiers` — flow
    through the existing `toQueryString` automatically
  - `SearchModifier`, `UploadAck` types
  - `listSearchModifiers()`, `uploadImageForSearch(file)` — the latter
    serializes a multipart body by hand (server-side, so no browser
    `FormData` shortcut available)

- **Known dev limitation (carried from Phase A)**
  - Real uploads from local dev still 502 at the inline embed step
    because Jina's workers can't fetch `http://localhost:9000/...`.
    The UI flow (file picker → server action → redirect → modifier bar
    renders → toggle modifiers) is fully exercisable; the actual
    ranked results land when MinIO is fronted by a public URL

- **Tests** — 3 new Playwright specs in `19-visual-search-modifiers.spec.ts`:
  - Hero "Search by image" affordance renders
  - `/search?image_upload_id=…` server-renders the anchor strip +
    modifier toolbar
  - Clicking a modifier toggles the URL param and flips `aria-pressed`
  - Tally: Playwright **28** (was 25). Rust 131 unchanged. Vitest 20
    unchanged

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

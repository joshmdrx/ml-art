# E2E coverage register

Authoritative map from **user-visible feature** → **Playwright spec** →
**status**. Read this before shipping a feature to figure out what
should be tested at Tier 2, and update it as part of the same commit
when you ship — see [`CLAUDE.md`](../CLAUDE.md) → "E2E coverage
discipline".

**Related:** [`TESTING.md`](../TESTING.md) covers the four-tier testing
posture (Rust integration / Vitest units / Playwright / CI); this file
is only the Tier-2 register.

---

## Legend

- ✅ **Covered** — a spec exercises the flow end-to-end
- 🟡 **Partial** — spec exists but doesn't cover the full flow
  (surface / assertion gap noted)
- ⏳ **Gap** — user-visible flow with no E2E coverage; **write one
  or explicitly defer here with a reason**
- 🚫 **Deliberately skipped** — infrastructure / cost / determinism
  reason means E2E isn't the right tier

Every ⏳ row is an implicit ticket. Move to ✅ when a spec lands, or
downgrade to 🚫 with the reason if you decide it's not worth it.

---

## Discovery + browse (anonymous)

| Feature | Spec | Status |
|---|---|---|
| Homepage renders hero + neighborhoods + recent grid | [`01-home`](../e2e/tests/01-home.spec.ts) | ✅ |
| Keyword search → results page | [`02-search-keyword`](../e2e/tests/02-search-keyword.spec.ts) | ✅ |
| Card → artwork detail + "More like this" | [`03-artwork-detail`](../e2e/tests/03-artwork-detail.spec.ts) | ✅ |
| Card → artist portfolio | [`04-artist-portfolio`](../e2e/tests/04-artist-portfolio.spec.ts) | ✅ |
| Neighborhood index → detail | [`05-neighborhoods`](../e2e/tests/05-neighborhoods.spec.ts) | ✅ |
| Unknown slugs → 404 | [`06-not-found`](../e2e/tests/06-not-found.spec.ts) | ✅ |
| Impossible filter → empty state | [`07-empty-state`](../e2e/tests/07-empty-state.spec.ts) | ✅ |
| Location filter narrows results | [`08-location-filter`](../e2e/tests/08-location-filter.spec.ts) | ✅ |
| FilterBar (medium / price / availability / location) | [`16-filter-bar`](../e2e/tests/16-filter-bar.spec.ts) | ✅ |
| Visual search modifiers (image_upload_id shell) | [`19-visual-search-modifiers`](../e2e/tests/19-visual-search-modifiers.spec.ts) | 🟡 no real upload — Jina stub gap |
| Geography map mode (`?map=1`) | [`20-geography-search-map`](../e2e/tests/20-geography-search-map.spec.ts) | 🟡 shell only — seed has no `artist_locations` rows |
| Artwork image viewer — click main opens lightbox, Escape closes | [`29-artwork-image-viewer`](../e2e/tests/29-artwork-image-viewer.spec.ts) | ✅ |
| Artwork image viewer — thumbnail-swap on multi-image work | [`43-artwork-image-viewer-thumbnails`](../e2e/tests/43-artwork-image-viewer-thumbnails.spec.ts) | ✅ uses seed's 2-image Crimson Field |
| Series detail on `/artists/[slug]/series/[seriesSlug]` (T-058.3) | [`42-series-detail`](../e2e/tests/42-series-detail.spec.ts) | ✅ smoke + unknown-slug 404, uses seed's `blue-period` |
| "For you" row on homepage (T-056.2) | — | 🚫 personalisation is behind `SEARCH_PERSONALIZE_ENABLED`, deterministic assertion hard; cover once flag is on |
| Refine-with-text on `/search` (T-082) | — | 🚫 feature-flagged off (`NEXT_PUBLIC_REFINE_ENABLED`) pre-launch; add when flipped on |
| Medium-taxonomy multi-value filter (T-073) | — | 🟡 covered generically by `16-filter-bar`; multi-value chip behavior not explicitly asserted |
| Size-band chips (T-070) | — | 🟡 same — chips exist under FilterBar tests but no explicit assertion |
| Currency-aware price filter — bucket click (T-080) | [`31-price-filter`](../e2e/tests/31-price-filter.spec.ts) | ✅ |

## Inquire

| Feature | Spec | Status |
|---|---|---|
| Anonymous inquire → dev verify link resolves | [`09-inquire-anonymous`](../e2e/tests/09-inquire-anonymous.spec.ts) | ✅ |
| Bogus verify token → not-found copy | [`11-inquiry-verify-bogus`](../e2e/tests/11-inquiry-verify-bogus.spec.ts) | ✅ |
| Signed-in inquire (no verify step) | [`13-inquire-signed-in`](../e2e/tests/13-inquire-signed-in.spec.ts) | ✅ |
| Studio inbox (two-pane, URL-driven `?id=`) | [`22-studio-inquiries-signed-in`](../e2e/tests/22-studio-inquiries-signed-in.spec.ts) | 🟡 predates 2026-07-01 UX rewrite — reload + selection persistence not asserted |
| Inbound email reply appears in thread (T-054) | — | 🚫 needs local SMTP + Worker; deferred to smoke tier |
| Unread inquiry badge on StudioSidebar (T-074) | [`49-unread-inquiry-badge-artist-signed-in`](../e2e/tests/49-unread-inquiry-badge-artist-signed-in.spec.ts) | ✅ uses the T-069 test-fixture seam (`POST /v1/testfixtures/inquiry`) |

## Save + collections

| Feature | Spec | Status |
|---|---|---|
| Signed-out Save → bounce to sign-in | [`10-save-signed-out`](../e2e/tests/10-save-signed-out.spec.ts) | ✅ |
| Signed-in Save modal — toggle + inline create | [`12-save-signed-in`](../e2e/tests/12-save-signed-in.spec.ts) | ✅ |
| `/collections` index (signed-in) | [`14-collections-signed-in`](../e2e/tests/14-collections-signed-in.spec.ts) | ✅ |
| Save modal membership awareness (T-029) | [`15-save-membership-signed-in`](../e2e/tests/15-save-membership-signed-in.spec.ts) | ✅ |
| Public share via `/c/[share_id]` (T-053) | [`24-collection-share-signed-in`](../e2e/tests/24-collection-share-signed-in.spec.ts) | ✅ |

## Follow + notifications

| Feature | Spec | Status |
|---|---|---|
| Signed-in follow toggle + persistence (T-052) | [`25-follow-signed-in`](../e2e/tests/25-follow-signed-in.spec.ts) | ✅ |
| Anon-pending-follow queue → replay after sign-in (T-052c) | [`45-follow-anon-queue`](../e2e/tests/45-follow-anon-queue.spec.ts) | ✅ mid-test Clerk sign-up; ~30s per run |
| Notification prefs read + toggle round-trip (T-068) | [`26-notification-prefs-signed-in`](../e2e/tests/26-notification-prefs-signed-in.spec.ts) | ✅ |
| One-click unsubscribe `/u/confirm` — bogus + missing token error copy | [`27-unsubscribe-token`](../e2e/tests/27-unsubscribe-token.spec.ts) | ✅ |
| Valid unsubscribe token → prefs flipped off | — | 🚫 needs token minted by API in-test; covered by Rust integration + smoke |
| New-works digest actually sends (T-052b) | — | 🚫 cron-driven; smoke-tier |

## Anon → signed-in bridge

| Feature | Spec | Status |
|---|---|---|
| `POST /api/me/merge-anonymous` fires once + marker set (T-033) | [`23-anon-merge-signed-in`](../e2e/tests/23-anon-merge-signed-in.spec.ts) | ✅ |
| Anon events attributed to user on sign-in (T-050.4) | — | 🟡 Rust integration covers the join; browser side not asserted |

## Studio (artist-side)

**Artist fixture:** `artist.setup.ts` mints a fresh Clerk user AND
drives them through the onboarding wizard end-to-end, producing a
user with an `artists.user_id` link. Metadata (email, display name,
slug) persists to `e2e/.auth/artist-meta.json` so downstream specs
can reach the fixture's public artist page. The `chromium-artist`
project picks up `*artist-signed-in*.spec.ts`.

**Test-fixture insert seam:** two direct-DB POSTs at
`/v1/testfixtures/artwork` + `/v1/testfixtures/inquiry`, wrapped by
`e2e/lib/fixtures.ts` (`createArtwork` + `createInquiry`).
Guarded by `WANDER_TEST_FIXTURES_ENABLED=1` — routes never register
in prod. Skip auth, moderation, embedding — lets specs seed world
state under the fixture artist without driving through Jina + S3 +
Clerk image-upload paths. Used by specs 49–51.

| Feature | Spec | Status |
|---|---|---|
| `/studio/settings` auto-provisions artist row (T-012 P1) | [`17-studio-settings-signed-in`](../e2e/tests/17-studio-settings-signed-in.spec.ts) | ✅ |
| `/studio/settings` bio edit persists across reload | [`47-studio-settings-edit-artist-signed-in`](../e2e/tests/47-studio-settings-edit-artist-signed-in.spec.ts) | ✅ |
| Onboarding review-step shows "View your profile" for already-active artist | [`48-onboarding-review-active-artist-signed-in`](../e2e/tests/48-onboarding-review-active-artist-signed-in.spec.ts) | ✅ |
| `/studio` portfolio for a fresh Clerk user (T-011 P3) | [`18-studio-portfolio-signed-in`](../e2e/tests/18-studio-portfolio-signed-in.spec.ts) | ✅ |
| Onboarding wizard identity step (T-012 P1) | [`21-onboarding-signed-in`](../e2e/tests/21-onboarding-signed-in.spec.ts) | 🟡 identity step only; steps 2–5 not covered |
| `/studio/series` non-artist redirects to onboarding (T-058.2) | [`28-studio-series-signed-in`](../e2e/tests/28-studio-series-signed-in.spec.ts) | ✅ |
| Studio sidebar nav (2026-07-01 UX rewrite) | [`46-studio-sidebar-artist-signed-in`](../e2e/tests/46-studio-sidebar-artist-signed-in.spec.ts) | ✅ 4 nav items + aria-current follows route (uses artist fixture) |
| Artwork edit modal — URL-driven lifecycle (`?id=<uuid>`) | [`50-studio-artwork-modal-artist-signed-in`](../e2e/tests/50-studio-artwork-modal-artist-signed-in.spec.ts) | ✅ modal opens on direct nav, Escape strips param |
| Series edit modal — URL-driven lifecycle (T-058.2) | — | ⏳ same shape as spec 50; add once we're actively touching /studio/series |
| Studio → public artist page link (2026-07-01) | — | 🚫 single link, low breakage risk |
| Publish nudge on incomplete draft (T-070) | [`51-publish-nudge-artist-signed-in`](../e2e/tests/51-publish-nudge-artist-signed-in.spec.ts) | ✅ draft → published without dimensions + medium_category fires the confirm dialog |
| Bulk image upload (T-011 P5) | — | 🚫 requires real upload pipeline + Jina stub |

## Admin (T-083 / T-084)

**Admin fixture:** `admin.setup.ts` signs up a fresh Clerk user whose
email ends with `-admin+clerk_test@example.com`. `is_seeded_admin_email`
matches that suffix against `WANDER_ADMIN_EMAIL_ALLOWLIST` (set to
`-admin+clerk_test@example.com` in `scripts/dev.sh` +
`.github/workflows/e2e.yml`), so the user is auto-promoted on first
authenticated request. Storage state lives in `e2e/.auth/admin.json`;
the `chromium-admin` project consumes it and picks up
`*-admin-signed-in.spec.ts`.

| Feature | Spec | Status |
|---|---|---|
| Non-admin visits `/admin` → 404 | [`34-admin-blocked-signed-in`](../e2e/tests/34-admin-blocked-signed-in.spec.ts) | ✅ |
| Admin sees `/admin` index tiles + queue links (T-083) | [`32-admin-index-admin-signed-in`](../e2e/tests/32-admin-index-admin-signed-in.spec.ts) | ✅ |
| `/admin/stats` renders tiles + funnel (T-084.1) | [`33-admin-stats-admin-signed-in`](../e2e/tests/33-admin-stats-admin-signed-in.spec.ts) | ✅ |
| Admin approve pending artist → row leaves pending queue (T-083) | [`35-admin-artists-approve-admin-signed-in`](../e2e/tests/35-admin-artists-approve-admin-signed-in.spec.ts) | ✅ mutates seed's `dora-pending`; local re-run needs seed reset |
| Admin override rejected image → row leaves rejected queue (T-083.3) | [`36-admin-images-override-admin-signed-in`](../e2e/tests/36-admin-images-override-admin-signed-in.spec.ts) | ✅ mutates seed's Carmen-linocut rejected row |
| `/admin/audit-log` renders (T-083.5) | [`37-admin-audit-log-admin-signed-in`](../e2e/tests/37-admin-audit-log-admin-signed-in.spec.ts) | ✅ tolerant of empty/non-empty state so spec ordering doesn't matter |
| Admin banner on non-active artist page (T-084.2) | [`38-admin-artist-banner-admin-signed-in`](../e2e/tests/38-admin-artist-banner-admin-signed-in.spec.ts) | ✅ uses seed's `edith-paused` |
| Non-admin visitor to non-active artist → 404 (T-084.2 inverse) | [`39-artist-non-active-blocked-signed-in`](../e2e/tests/39-artist-non-active-blocked-signed-in.spec.ts) | ✅ |
| Admin decline pending artist (confirm-dialog path) | [`40-admin-artists-decline-admin-signed-in`](../e2e/tests/40-admin-artists-decline-admin-signed-in.spec.ts) | ✅ uses seed's `franz-pending` |
| Admin pause / unpause round-trip | [`41-admin-artists-pause-admin-signed-in`](../e2e/tests/41-admin-artists-pause-admin-signed-in.spec.ts) | ✅ uses seed's `greta-active` |

## OG cards + metadata (T-051)

| Feature | Spec | Status |
|---|---|---|
| Artwork OG image renders at expected URL | [`30-og-cards`](../e2e/tests/30-og-cards.spec.ts) | ✅ meta URL emitted + fetch returns image/* |
| Artist OG image renders | [`30-og-cards`](../e2e/tests/30-og-cards.spec.ts) | ✅ same for `/artists/[slug]` |
| Public-collection page + OG image | [`44-public-collection-og`](../e2e/tests/44-public-collection-og.spec.ts) | ✅ uses seed's `test-share-alice` public collection |

## Prod smoke (T-075)

Prod smoke lives in `web/scripts/smoke.ts`, is read-only, and runs
against a live env. It's a separate discipline from local Playwright
and isn't tracked in this register.

---

## Editing this file

- **On shipping a new user-visible feature**: add a row. If you didn't
  write a spec for it, mark ⏳ (or 🚫 with a one-line reason). Empty
  cells are not allowed.
- **On writing a new spec**: mark the row ✅ and link the spec.
- **On finding a legitimate reason not to E2E-test something**: change
  the status to 🚫 and write the reason. Not writing a spec because
  it's tedious is a ⏳, not a 🚫.
- Keep spec counts in [`TESTING.md`](../TESTING.md) → "Acceptance"
  in sync when you land a batch.

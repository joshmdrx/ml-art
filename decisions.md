# Decisions Log

Chronological log of significant architectural and product decisions.
Lightweight by design — heavier ADRs are overkill at this stage. If an entry
gets revised, link the original and add a follow-up entry rather than editing.

Format:

```
## YYYY-MM-DD — short title
**Context:** what was the situation
**Decided:** what we chose
**Alternatives:** what else we considered
**Why:** rationale
**Reversibility:** Low / Medium / High — how hard is this to undo later
```

---

## 2026-06-30 — T-083: admin surface — column over table, audit-before-mutate, layout-level 404 gate

**Context:** Three queues need human approval before going public —
artists (`status='pending'`), images (`moderation_status='rejected'`
overrides), and (soon) venues (T-081). All three were being handled
via direct psql edits. The same shape kept reappearing in scope
discussion, so the foundational pieces are worth getting right
before they multiply.

**Decided:**

1. **`users.is_admin BOOLEAN` column.** Pre-existing on the table
   (0001_init.sql); kept rather than swapped for a separate
   `admins` table with roles. Today one admin, ever-growing
   manually; the column is cheaper for that shape. A roles table
   lands when (a) we have multi-admin AND (b) we have at least two
   distinct sets of permissions worth distinguishing.

2. **Auto-promote on first sign-in from a hardcoded `ADMIN_EMAILS`
   list.** `core::auth::upsert_user` checks the list on INSERT and
   seeds `is_admin=true` for matching emails. The migration's
   bootstrap UPDATE handles the inverse case where the user signed
   in before the constant landed. Either ordering converges. The
   ON CONFLICT branch OR's the seed flag against the existing
   value — manual promotions are never overwritten by a re-seed.

   Rejected: an `admin_seed_emails` table you can INSERT into. The
   hardcoded list is in version control, reviewable in PRs, and
   accompanies a deploy — qualities you want for an admin grant.
   Add a table when admins exceed ~5.

3. **`admin_audit_log` from day 1.** Retrofitting audit later is
   awful — every existing admin endpoint would need surgery to add
   the row. Up front it's one extra INSERT per mutation. NULL
   `admin_user_id` represents a system action (the auto-promotion
   itself doesn't audit; future scheduled jobs that mutate state
   will). Generic `before_jsonb` / `after_jsonb` columns avoid
   per-target schema churn.

4. **Audit BEFORE the mutation it audits.** Not in a single
   transaction with the UPDATE. If the UPDATE fails (e.g. illegal
   transition, row already gone) the audit still records the
   admin's intent. "Tried to approve at T but the row was already
   gone" is itself audit signal — losing it because the UPDATE
   no-op'd would be wrong. Cost: an audit row can exist without a
   corresponding state change. Tradeoff accepted; the alternative
   (atomic transaction) loses the intent signal.

5. **403 from the API, 404 from the web layer.** `AdminUser`
   extractor returns 403 for non-admins (`/v1/admin/*` is the
   correct address; you just can't operate on it). The web
   `/admin/*` layout returns `notFound()` for non-admins so the
   admin surface is invisible to anyone who doesn't already know
   it exists. Two different lies for two different audiences.

6. **Idempotent re-application skips the audit.** Approving an
   already-active artist returns the row, no audit row. Same for
   override-an-already-approved image. "I double-clicked approve"
   shouldn't clutter the log.

7. **Illegal source-state transitions are 409 Conflict.** Approving
   a `rejected` artist isn't allowed — the admin must re-pending
   them first (separate affordance, not yet built). Better than
   silent no-op because the UI would otherwise show a stale state.

8. **Web actions: server-action wrappers, useConfirm + toast
   surface.** Mirrors the studio side. Destructive transitions
   (decline, pause, image override) go through `useConfirm`;
   every action surfaces via `toast.success` / `toast.error`. ESLint
   bans `window.confirm`; the convention is universal across the
   web layer.

**Alternatives:**

- **Open-self-listing for new admins:** rejected. Same reason
  artists go through `status='pending'` rather than self-publishing
  on signup — friction is the feature.
- **Scoped roles on day 1** (`admin_roles` table with bitfields):
  rejected. Adds complexity for no concrete use case yet. Single
  admin pre-launch; add when there's a second admin who shouldn't
  see all queues.
- **Inquiry abuse / report queue in v1:** rejected. Real once we
  have signed-up users at volume; not before. The shape will
  follow the artist / image queue patterns when it lands.
- **Hard-delete declined artists:** rejected. `status='declined' +
  decided_at` preserves the row so an appealing artist can be
  re-pending'd; data is never destroyed silently.
- **Audit retention windowing:** deferred. The table will be tiny
  in row count for years (single-digit admin operations per day
  at v1 scale). Layer in a TTL or partition once daily volume
  justifies it.

**Reversibility:** Medium for the column choice + admin email
constant (migrating to a roles table is a schema change + code
sweep but mechanical). Low for the audit log: once written, those
rows are the system's official memory of "who did what" and we
can't go back. That's the point.

---

## 2026-06-30 — T-082: refine-with-text — fourth RRF channel, not anchor mutation

**Context:** Visual search has carried two ranking-modifier primitives
since T-010:
- The anchor vector itself (one of: `q` text embed, `image_upload_id`,
  `seed_artwork_id`) drives the semantic channel.
- Fixed-vocabulary modifiers (moodier / warmer / quieter / …) shift
  the anchor at α=0.8 via precomputed δ-vectors anchored on WikiArt.

Both serve "I want results biased toward X." Neither covers "I want
results like this image **but more abstract**" where X is open-ended —
the user has a phrase, not a δ in our fixed registry.

**Decided:** Add a fourth RRF channel `refine_ranked` parallel to
keyword + semantic + taste. `?refine=TEXT` embeds via Jina (same path
as `?q=`), the resulting vector contributes a `ROW_NUMBER() OVER
(ORDER BY embedding <=> refine_vec)` ranking, blended into rrf_score
via the same `1/(60+rk)` shape.

Refine **joins the candidate-contribution clause**
(`k.id IS NOT NULL OR s.id IS NOT NULL OR r.id IS NOT NULL`), unlike
taste which only re-ranks. The user typing "abstract" wants us to
pull abstract works in *even if the visual anchor missed them* —
that's the whole point of refine. Taste is "given these candidates,
rank by what I like"; refine is "consider these too, then rank."

Refine is silently dropped when no primary signal is set (no q, no
image_upload_id, no seed_artwork_id). Promoting refine into the
keyword channel in that case would be technically possible but
behaviour-confusing: `?refine=cats` would silently become `?q=cats`
with no explanation. Better to no-op so the URL contract is honest.

**Alternatives:**

- **Pre-blend refine into the anchor at some α** (like modifiers).
  Rejected: refine is *additive* preference, not a *shift* in style.
  Modifiers' WikiArt-validated δ-vectors are calibrated for shifting
  along an axis; arbitrary free-form text isn't. Pre-blending would
  also lose the per-channel ranking guarantee RRF provides — a single
  weak refine signal would corrupt the strong visual anchor.
- **Boost the keyword channel for terms in refine.** Rejected:
  refine is semantic, not lexical. "Abstract" the concept and
  "abstract" the substring are different signals; refine wants the
  former, the keyword channel offers the latter.
- **Replace the fixed-vocabulary modifiers entirely.** Rejected:
  modifiers are one-click quick deltas with well-tested behaviour;
  refine is the free-form sibling for cases modifiers don't cover.
  Different ergonomics, both have value. Keeping both also gives us
  a place to land "common refines" as new fixed modifiers later
  without breaking either path.
- **Compose refine over modifiers (δ-vector form).** Rejected: no
  closed-form way to compute a δ-vector from arbitrary text without
  a reference anchor (modifiers' deltas are calibrated against
  WikiArt centroids — free-form text has no equivalent calibration).

**Why:** RRF fuses ranked lists without requiring weight tuning across
heterogeneous signals — that's its whole pitch. Adding a fourth
channel costs one CTE + one term in the fusion expression, naturally
calibrated by `k=60`. The semantic + taste channels already proved
the shape; refine is the third repetition of the same pattern.

A defensive 500-char cap guards against malicious callers spending
our Jina budget on long inputs; one embed per request, same as the
existing `q=` and modifier paths.

**Reversibility:** Medium. The channel itself is one CTE — removing
it is mechanical. The user-facing URL contract `?refine=…` is the
sticky part: deprecating it would be a breaking change for any
bookmarked URLs.

---

## 2026-06-25 — T-057: algorithmic neighbourhoods — HDBSCAN over normalised CLIP, evocative Claude labels, pure-rebuild persistence

**Context:** `/neighborhoods` was populated by a single hand-curated
`test-vibes` row. Schema has supported algorithmic clusters since
launch (`kind='semantic'`, `cluster_centroid vector(1024)`,
`representative_artwork_ids uuid[]`). Time to actually populate it —
the schema being ready meant the only open questions were operational.

**Decisions:**

1. **`min_cluster_size=30`.** Picked the bottom of the "tight enough
   to feel coherent, loose enough to find more than 2 groups" range.
   With ~few-hundred published artworks, smaller would over-fit; much
   larger would collapse to ≤2 noisy macro-clusters.

2. **HDBSCAN with `metric='euclidean'`, `cluster_selection_method='eom'`,
   over L2-normalised embeddings.** Considered cosine directly but the
   `hdbscan` package's metric-tree fast paths assume a true metric;
   euclidean on a unit-norm vector is monotone in cosine distance,
   so the cluster shapes are identical and the math is cleaner. `eom`
   over `leaf` because we want the most stable clusters, not the
   maximum-density-peak ones.

3. **Evocative names over taxonomic.** Asked Claude for "Quiet
   Mornings" / "Saturated Geometry" style names instead of "Pastel
   Still Life" / "Abstract Geometric Acrylic". The latter is what a
   filter chip should be; the former is what a *destination* should
   be — the whole UX value of neighbourhoods over filters.

4. **Pure-rebuild, no Hungarian-matching of old → new centroids.**
   Future weekly runs will drift cluster shapes. Mature systems map
   old slugs to new clusters by greedy nearest-centroid match to keep
   bookmarks / shareable URLs stable. We don't have shareable bookmarks
   yet (pre-launch) and slugs are derived from labels anyway, so we
   accept slug churn for now. Revisit when first real user bookmarks a
   neighbourhood URL.

5. **Drop the `test-vibes` row on every run (`--prune-test-vibes`).**
   It served its purpose validating the page renders; algorithmic
   clusters supersede it cleanly.

6. **Per-cluster fallback on label failure, not whole-run abort.** One
   Claude reply with broken JSON should produce `Cluster 7` /
   `Placeholder description` for *that* cluster, not lose the other 11.

**Reversibility:** High. The whole pipeline is one script + one DELETE.
Tuning parameters is a re-run away; switching algorithms is a file
rewrite. No schema commitments — `kind='semantic'` is just a string.

---

## 2026-06-26 — T-061: calibrator — picks-as-events, no anonymous profile store

**Context:** T-056 "For you" and T-060 Discover Weekly both need an
established taste vector to be useful, and T-055 only builds one once
a user has accumulated saves/inquiries/views — useless for someone in
their first session. T-061 fills that gap with a 5-pair "this or
that" UX before they've done anything else.

**Decisions:**

1. **Picks live as events, not a separate anonymous taste-vector
   store.** The TODO said "seeds the anonymous taste vector" but the
   `user_profiles` schema FKs `user_id` into `users` — anonymous
   profiles need either a schema migration (nullable user_id + add
   anonymous_id), a sibling table, or a client-side store. All three
   add infra. Events-only is the lightest path: a `calibration_pick`
   event with `artwork_id` in `properties` feeds the same T-055
   refresh path as a save or view. Anon picks fold into the user's
   taste vector at sign-in via T-033 anon-merge. Pre-launch when
   conversion isn't measurable, the tradeoff is "anon visitor who
   never signs up loses calibration" — accepted as cheap to revisit
   if conversion data later shows it's worth the anon-profile infra.

2. **Weight 2.0 — between view and save.** A calibration pick is an
   explicit "this over that" choice (stronger than a passive view,
   weight 0.5) but with no context — they didn't write text, didn't
   click through, didn't inquire (weaker than a save, weight 3.0).
   2.0 splits the difference. Tune as engagement data arrives.

3. **Greedy farthest-first pair selection, not maximum-weight
   matching.** Max-weight matching gives the globally optimal
   "farthest-pairing-on-average" but at `O(n³)` with the Hungarian
   algorithm. Greedy is `O(n²)` and gives visually-distinct pairs
   that are fine for the UX. With only ~14 clusters in prod the
   distinction is invisible; revisit if cluster counts grow past ~50.

4. **localStorage flag, not server-side "calibrated" state.** The
   panel hides via `localStorage["wander:calibrator"] ∈ {"done",
   "skip"}` on subsequent visits. Cross-device hand-off is a non-
   feature pre-launch; doing it server-side would mean an extra
   round-trip on every homepage load. Trade accepted.

5. **SSR the pairs, bridge only the POST.** GET happens during the
   page render — no client request, faster TTI for the new visitor.
   POST goes through `/api/calibrate/pick` for the same cookie-
   forwarding reason as `/api/events`. Matches the T-050.3 split.

**Reversibility:** Very high. Server-side it's one new module +
one EventName variant + one weight constant. Client-side it's one
component + one route. localStorage flag is a single key. Removing
the feature is "delete the files, remove the homepage import."

---

## 2026-06-29 — Studio modals: URL-driven lifecycle for multi-step flows

**Context:** T-058's `SeriesEditModal` had a multi-step
create-then-attach-artworks flow that I first built with local
`useState` + a `useRef`-based "did the parent just re-open us?"
detection. It broke twice: tab wouldn't flip after create; later, an
edit-mode Save left the modal open because two paths to `closeModal`
were firing in the same tick and racing. Both bugs went away when we
moved the modal's lifecycle (open/closed, current tab, current
series) into the URL as `?id=` + `?tab=` and let the parent drive
state transitions via `router.replace`.

Ported `ArtworkEditModal` to the same shape immediately after for
consistency.

**Decisions:**

1. **URL params are the source of truth for multi-step modal
   state.** Multi-step here means *the modal lifecycle has
   transitions the user can think of as distinct stages* — create →
   edit-the-thing-you-just-made; details-tab → artworks-tab; etc.
   These map naturally onto URL state because the steps are real
   user-visible states. Useful side-effects (shareable links,
   refresh-friendly, back-button closes the modal) come for free.

2. **Parent owns the URL; modal is dumb.** Modal receives `target`,
   `tab`, `onTabChange`, `onSaved`, `onClose` as props and calls them
   on events. Parent translates events to `router.replace`. Modal
   never closes itself — that previous design produced races between
   the modal's local state and the parent's URL state.

3. **`router.replace`, not `router.push`, for in-modal
   transitions.** Pushing every intra-modal step bloats history; the
   back-button stops should be "modal closed" vs "modal open with
   target X" — not "tab 1 → tab 2 → tab 3 → tab 1". Replace keeps
   the URL accurate without flooding history.

4. **Preserve other params via a `urlWith` helper.** Modal lifecycle
   transitions shouldn't reset the artist's `?status=draft` filter.
   The helper copies the current `searchParams` and applies a sparse
   patch (set / delete keys). Canonical implementation in
   `StudioPortfolio.tsx`.

5. **Mutually-exclusive save paths.** `closeAfter=true` →
   `onClose()` only (close path). `closeAfter=false` → `onSaved()`
   only (URL-advance / state-lift path). The first iteration fired
   both for `closeAfter=true`, producing a double `router.replace`
   that visibly raced. Fix: single dispatch from a single source.

6. **In-modal state-lift short-circuit on URL flip.** When the URL
   advances from `?id=new` to `?id=<new-uuid>` after a create, the
   modal already has the just-created detail in memory. The load
   effect short-circuits if `load.detail?.id === target` — avoids
   a "Loading…" flash from a wasteful refetch.

**When NOT to use:** simple one-shot modals (yes/no confirms via
`useConfirm`, anonymous inquiry form, save-to-collection picker).
URL state overhead is real; reserve it for surfaces where the
sharable / refresh-friendly behaviour is worth it.

**Documented in:** `docs/ui-patterns.md` → "Multi-step modals — drive lifecycle from URL params".

---

## 2026-06-29 — T-058: series — one series per artwork, multi-select over drag

**Context:** Artists work in series — projects, themes, year-cohorts —
and the flat grid we ship today is hostile to how they actually
present their practice. T-058 adds the data model + studio UI +
public surface for grouping works under shareable series headers.

**Decisions:**

1. **One series per artwork (FK), not many-to-many.** Spec called for
   this and we kept it. The trade is real but small: an artwork that
   genuinely belongs to two series (e.g. "Quiet Mornings" and a
   gallery's curated retrospective) can't sit in both. Migration
   path if this becomes a problem is a junction table; the public
   API shape would change minimally (`series` array vs single).
   For now, single-FK keeps the data model tractable and lets the
   artwork PATCH expose `series_id` as a one-line three-state field.

2. **Per-artist unique slug, not globally unique.** Two artists can
   both have a "blue-period" series. URLs are namespaced
   (`/artists/:artist-slug/series/:series-slug`) so there's no
   collision in the wild. Collisions within an artist surface as
   409; we let the artist re-title rather than auto-suffix (`-2`).
   Cleaner from the artist's perspective — they pick the name they
   want, the system confirms.

3. **Multi-select checkbox grid over drag-to-assign.** The TODO
   originally suggested drag-to-assign; in design I traded it for a
   checkbox grid in the series-edit modal. Reasons: (a) one-direction
   mental model (manage from the series side) vs drag's two-way
   ambiguity, (b) bulk operations are first-class via the
   `PUT /:id/artworks` endpoint that replaces membership atomically,
   (c) drag UX is harder to build well — a half-built drag is worse
   than a complete checkbox grid. The per-artwork dropdown
   (deferred) handles the inverse "this one work belongs to series
   X" path.

4. **Empty series studio-only.** Hidden from the public list + the
   detail endpoint returns 404 for them. Lets artists prepare a
   series (title, statement, cover) before any works are assigned
   without that empty-state leaking publicly.

5. **No NFKD unicode fold on slugify.** Café → "caf-visions" rather
   than "cafe-visions"; mirrors the same call we made on T-061's
   calibrator. Avoids a new dep for a marginal UX win — artist can
   manually edit if they care.

6. **Cover image = picker from own works, not separate upload.**
   Avoids the upload pipeline + S3 path + moderation flow for what
   is just a representative image. Null falls back to the first
   member's primary image so empty/uninitialised series still have
   a visual identity.

7. **Soft-delete on series, hard-clear on member artworks.** When
   an artist deletes a series, the `series.deleted_at` flips but
   the FK doesn't cascade — we explicitly clear `artworks.series_id`
   in the same transaction. Artworks survive intact, just
   un-series'd. Matches the spirit of the FK's `ON DELETE SET NULL`
   for the hard-delete path while keeping the soft-delete option for
   recovery if we want it later.

**Reversibility:** Very high. The migration is additive (new table +
nullable column); rolling back is a `DROP TABLE series; ALTER TABLE
artworks DROP COLUMN series_id`. The studio + public surfaces are
new pages — no existing flows changed. The artwork DTO gained a
`series_id` field which is nullable; old clients ignore it.

---

## 2026-06-29 — T-056: personalised re-rank — RRF third channel, default-off, taste only tilts

**Context:** Events flow (T-050), taste vectors compute (T-055),
clusters seed the cold start (T-057 + T-061). T-056 is where users
actually see the difference — a personalised "For you" row on the
homepage and a taste-tilted search ranking. Three sub-commits, each
with its own design call.

**Decisions:**

1. **Lowered `MIN_INTERACTION_COUNT` from the spec's 10 to 5.** The
   TODO threshold was a guess; what matters is the cold-start UX. A
   finished T-061 calibrator session = 5 picks, so the threshold-5
   choice means "asking the user 5 questions on first visit is
   enough to unlock personalisation." That's a tight product loop.
   Threshold-10 would require 5 *more* events after the calibrator,
   which most users won't have generated in their first session.

2. **RRF blend default-off — feature is dark until A/B-ready.** The
   risk profile of T-056.3 is materially different from T-056.1/.2.
   The endpoint + homepage row are additive surfaces (eligible
   users get a new row, no one else sees a change). The RRF blend
   changes search ordering for every signed-in eligible user on every
   query — silent quality regressions are the failure mode and they're
   hard to spot. So we ship the code, build the toggle, and don't
   flip it until we have an evaluation harness. Operator switch
   (`SEARCH_PERSONALIZE_ENABLED` env) + per-request override
   (`?personalize=on|off`) give us the levers without redeploying.

3. **Taste is a TILT, not a CHANNEL on equal footing.** The WHERE
   clause keeps `(k.id IS NOT NULL OR s.id IS NOT NULL)` — a result
   must hit keyword OR semantic to be a candidate, then taste tilts
   the ranking among those candidates. Without this, an abstract-
   expressionism enthusiast searching "watercolour landscape" might
   see abstract expressionism creep into the top results just because
   it matches their taste. Bad UX. The "blended alongside" wording in
   the TODO is rendered as "added to the score, doesn't expand the
   candidate set."

4. **Empty-CTE pattern over conditional SQL.** When taste is off, the
   `taste_ranked` CTE is `SELECT NULL::uuid AS id, NULL::bigint AS rk
   WHERE false`. The final SELECT joins it unconditionally and the
   `COALESCE(1.0/(60 + t.rk), 0)` evaluates to 0. Default-off path is
   byte-identical to pre-T-056.3 search SQL. Mirrors the existing
   `semantic_ranked` empty-CTE pattern used when there's no semantic
   anchor — one less special case to reason about.

5. **Random shuffle in SQL for `for-you`, not deterministic-per-day.**
   The spec says "small jitter (random rank ±5 on top 50)" — we
   implemented as `ORDER BY random() LIMIT 12` over the top-50.
   Slightly stronger than ±5 but cleaner. The trade is that the row
   shuffles every page-reload on the same day, which might feel
   chaotic. Noted as a follow-up: swap for a `md5(id || user_id ||
   current_date)` shuffle if return-rate data suggests stability >
   freshness.

6. **Optional `AuthedUser` extractor, not required.** Search must
   stay open to anonymous callers. Making the extractor optional via
   `auth: Option<AuthedUser>` (rather than threading auth through the
   handler explicitly) lets the existing extractor handle the JWT
   parsing — no new code paths.

**Reversibility:** Very high. T-056.1 + .2 are additive surfaces; deleting them is `git revert`. T-056.3's default-off behaviour means flipping the env back to false (or just leaving it false) reverts to pre-change ranking. The empty-CTE pattern means there's no schema migration or job-queue change to roll back.

---

## 2026-06-26 — T-055: user taste vector — weighted-decayed L2-normalised sum, two-stage queue fan-out

**Context:** Events flowing (T-050) + cluster centroids in place
(T-057) unblock the first real ML personalisation engine. The
`user_profiles.taste_embedding` column has existed since migration
0006 with the HNSW index ready; we just needed something to write to
it. Every downstream retention feature (T-056 re-rank, T-060 digest,
cluster-of-a-user, T-061 calibrator merge) reads from this vector.

**Decisions:**

1. **Weighted-sum + L2-normalise, not EWMA-over-time.** The TODO
   originally said EWMA but on reflection that's redundant: time decay
   is already baked into per-event weights via
   `0.95^(weeks_old)`. EWMA would just be a second decay on top. The
   weighted-sum-with-decay formulation is the same math expressed
   more directly, and easier to reason about when tuning weights.

2. **Per-event weights live as `pub const` in the module, not in
   config.** They're tuning knobs the ML pipeline owner adjusts, not
   ops-time deployment config. Recompile-to-change is fine for that
   audience and keeps the per-event weight table in one obvious place.

3. **Negative weight on unsave, mirroring the save weight.** A user
   who saves then later unsaves should net to ~zero (modulo decay),
   not "save × 2." The `base_weight` function returns a signed value;
   the accumulator treats it as a vector subtraction. Modelled the
   same way for any future toggle events.

4. **Sub-noise-floor norms return None.** If a user has a single very-
   old view event, the decayed magnitude is essentially zero and the
   normalised direction is meaningless noise. Skip the write entirely
   rather than persist a misleading vector — downstream surfaces gate
   on `taste_embedding IS NOT NULL` already.

5. **Two-stage fan-out via the jobs queue, matching the T-052b digest
   pattern.** `Kickoff` scans + enqueues, `Refresh` does per-user
   work. Both run on the existing queue infra (Postgres locally, SQS
   in prod). No new cron infrastructure invented; reuses what's there.

6. **No anonymous-user profiles in v1.** The TODO said "anonymous
   users get refreshed too (keyed by `anonymous_id`)" but the schema
   makes `user_id` a PK FK into `users` — anonymous profiles need
   either a schema migration or a separate table. Deferred to T-061
   (the calibrator), where anonymous taste-vector seeding has a
   natural home alongside the cold-start UX. Pre-signin taste isn't
   lost: events keep flowing and the T-033 anon-merge handler links
   them to the user record at sign-in.

7. **Cron not live.** Same deferral as T-057's
   `neighborhoods-build` — both ML batch jobs wait for real users to
   start onboarding. The kickoff handler + `users_with_recent_activity`
   are ready; only the EventBridge → SQS schedule is unbuilt.
   Triggered manually via `jobs-worker --enqueue` until then.

**Reversibility:** Very high. Weights are constants; decay rate is a
constant. Schema-side we're only writing to existing columns. Killing
the cron stops refreshes; setting `taste_embedding=NULL` resets a
user. The whole pipeline is one module + 2 JobEvent variants.

---

## 2026-06-26 — T-057 tuning pass: leaf/15/2, Anthropic-vs-Groq bake-off

**Context:** First end-to-end run of the T-057 pipeline (above) against
the live corpus surfaced two issues the design discussion didn't catch.

**Problem A — clustering collapsed.** `eom` + `min_cluster_size=30`
produced 2 clusters from 2000 artworks: one 856-artwork "Western
figurative everything" mega-bucket and one 48-artwork ukiyo-e cluster.
~55% of the corpus classified as noise. The `eom` selection biases
toward fewer, more-stable clusters in the condensed tree — fine in
abstract, but on hierarchical CLIP-embedding data it merges entire
branches up.

**Tuning sweep (via the new `--preview` mode, which clusters without
labelling — single API-free run per config):**

| method | mcs | min_samples | clusters | noise |
|---|---|---|---|---|
| eom  | 30 | default(=mcs) | 2  | 55% |
| leaf | 30 | default       | 3  | 81% |
| eom  | 30 | 3             | 6  | 70% |
| eom  | 30 | 1             | 2  | 26% (one 1445-bucket) |
| leaf | 20 | 2             | 9  | 75% |
| leaf | 15 | 3             | 11 | 76% |
| **leaf** | **15** | **2** | **14** | **74%** |

**Decided:** `cluster_selection_method='leaf'`, `min_cluster_size=15`,
`min_samples=2`. 14 clusters with smooth size decay (90 → 15), no
mega-bucket, every cluster maps to a recognisable visual neighbourhood
in eyeball-check (cubism w/ instruments, pointillist landscapes,
ukiyo-e, Renaissance religious, Symbolist mountains, AbEx, etc.).
The 74% "noise" rate isn't bad: those artworks still appear in search
and on artist pages — they just don't belong to a discrete
neighbourhood. Honest signal over forced assignment.

The earlier `eom`/30 decision (#1–#2 above) is preserved for context;
this entry supersedes it.

**Problem B — provider lock-in vs cost.** Original ship was Anthropic-
only. User raised the cost question. Ran a head-to-head:

- Same clusters, same prompt, sample size 5 for both.
- Anthropic `claude-sonnet-4-6`: ~$0.05/run, ~30s.
- Groq `meta-llama/llama-4-scout-17b-16e-instruct`: ~cents/run, ~5s
  (modulo free-tier 429s, so the retry loop is necessary).

**Result:** Anthropic clearly wins on register. Same ukiyo-e cluster:
Anthropic says *"Silk and Shadow — Women in layered kimono inhabit
intimate moments, their elaborate dress and quiet gestures suspended in
soft, cream-toned space."* Groq says *"Elegant Japanese Portraits —
Classical depictions of refined Japanese women in traditional dress."*
Both accurate; Groq lands in the museum-caption register while
Anthropic delivers the discovery-cue voice the design asked for
("cream-toned space" vs "traditional dress" tells the story).

**Decided:** Anthropic stays the default. Groq is wired up behind
`--provider groq` as the cheap/fast iteration lane (prompt tweaks, full
re-runs while tuning) and as a hot fallback if Anthropic ever rate-
limits or has an outage. Pulling in Groq cost ~80 lines of code +
adding `pgvector` + `requests` to the `neighborhoods` extras; cheap
optionality.

**Hidden gotchas surfaced:**
- The `pgvector` adapter must be `register_vector(conn)`-ed or the
  `embedding` column comes back as a string (e.g. `"[0.1,0.2,…]"`) and
  `np.array(...)` silently builds a ragged object array that
  HDBSCAN can't fit. Added the register call right after `psycopg.connect`.
- Groq's vision endpoints cap at 5 images per request. Default sample
  size is now per-provider (12 for Anthropic, 5 for Groq) rather than
  a global constant.
- Groq's free tier rate-limits aggressively — retry-with-backoff on
  429 + 5xx is a 10-line addition, but skipping it makes any multi-
  cluster run fail.

**Reversibility:** Same as the original — script + DELETE. Provider
selection is per-run flag, sample size is per-run flag, cluster
config is per-run flag. Nothing fixed in schema.

---

## 2026-06-25 — T-050: behavioural events writer — single-table, queue-mediated, server-derived identity

**Context:** Every ML retention feature (T-055 taste vector, T-056
re-rank, T-057 neighbourhoods, T-060 digest, T-061 calibrator) was
blocked on event data flowing. The `events` table has existed since
launch but no writer. Time to wire it.

**Decisions:**

1. **Single `events` table with `event_name` discriminator + jsonb
   `properties` + jsonb `context`.** Considered per-event-type tables
   (better column hygiene, can't add an event without a migration).
   Rejected: flexibility-now > schema-now. We can partition later
   (T-016) without changing call sites. Spec'd in migration 0006;
   shipped as-is.

2. **Emit goes through the existing jobs queue** (`JobEvent::EventLog`),
   not a direct Postgres INSERT from handlers. The handler in
   `core::jobs::handle` owns the storage destination. Considered direct
   INSERT (one fewer hop, less infra), rejected because the storage
   abstraction is exactly what enables the future Postgres → S3 Parquet
   migration (decisions.md 2026-06-17). Emit sites never see the
   destination. SQS cost is negligible at this scale (~$0.10/month at
   projected volume).

3. **Best-effort emit, never blocks the request.** `events::emit`
   logs at WARN on enqueue failure and swallows. Analytics breakage
   must never propagate to a real user. Synchronous (not `tokio::spawn`)
   so the event is in the queue before the response leaves — Lambda
   freeze on response-return would lose the spawned task. SQS sendMessage
   latency is single-digit-ms; invisible to the user.

4. **Server-side identity, never client-supplied.** The `/v1/events`
   POST endpoint reads `anonymous_id` from the cookie and `user_id`
   from the Bearer token. Body-supplied identity fields are ignored.
   A malicious client can spoof their OWN anon events but can't
   attribute events to other identities.

5. **Closed allowlist for client-side names.** Only `modifier_applied`
   + `inquiry_started` accepted from `/v1/events`. Server-only events
   (`artwork_saved`, `inquiry_submitted`, etc.) stay server-only.
   Letting clients forge `artwork_saved` would poison the taste vector;
   the allowlist is the cheapest defence.

6. **PII (`context.ip` + `context.user_agent`) IS stored.** Considered
   omitting / hashing. Rejected — the data is genuinely useful (region-
   level analytics, abuse detection, session reconstruction) and we can
   choose to hash on read later. Documented as PII; retention + DSAR
   are deferred to a separate ticket (needs privacy policy +
   cookie-consent banner work first).

7. **All-or-nothing on batch validation.** Any name in a `/v1/events`
   batch failing the allowlist rejects the whole batch. Considered
   partial-success (commit valid, drop invalid). Rejected — half-state
   is harder to reason about than a clean reject + client retry.

8. **Page-1 only for `search_executed`.** Paginated scrolls don't
   re-emit; we treat search as a single intent. Considered "one per
   page" for richer engagement signal. Rejected — duplicate events
   inflate result-count averages without telling us anything new.

**Reversibility:**
- Schema: reversible. Drop columns / table; nothing else depends on it yet.
- Variant: reversible. Removing `JobEvent::EventLog` is a code change.
- Allowlist: reversible. Adding a name is additive.
- Emit sites: reversible per-handler.
- PII storage: reversible — `UPDATE events SET context = context - 'ip' - 'user_agent'` truncates after the fact if we decide differently.

**Alternatives considered:**
- *Per-event-type tables.* Rejected for migration overhead.
- *Direct Postgres INSERT from handlers (skip queue).* Rejected for
  storage-destination encapsulation.
- *Hashing IP on write.* Rejected for v1 — we'd lose analytic
  granularity now and the data isn't user-exposed.
- *`session_id` populated from a cookie.* Deferred — current cookie
  setup gives us anon_id (sufficient for "one user's journey")
  without needing a separate session boundary. Add when we have a
  product need for it (probably never).
- *Event-name strings hand-typed instead of enum.* Rejected — typo
  protection + closed-schema serde validation in one go.

---

## 2026-06-22 — T-071: feedback + dialog primitives (sonner toasts, useConfirm hook, FieldError, JS-only form validation)

**Context:** Real-artist testing (T-070 launch) surfaced three
consistency cracks in the studio UI within minutes:

1. Two error styles on the same form — HTML `min`/`max` attributes
   produced native browser tooltips, custom JS validation rendered
   inline red text. Same screen, two patterns.
2. `window.confirm` for the "publish without dimensions?" nudge —
   every other modal in the app uses Radix Dialog; this one fell
   back to native.
3. Edit modal stayed open after a successful save. Surprising. No
   feedback (no toast either).

Each is a one-line fix in isolation. Bundling them creates the
opportunity to establish patterns, build the missing primitives, and
ESLint-enforce the most fragile ones — so the next contributor
inherits the convention instead of relitigating it.

**Decisions:**

1. **`<FieldError message={…} />`** is the single inline-error primitive.
   `role="alert"`, `text-xs text-red-600`, returns null on empty. All
   form fields with JS validation use it; nothing inlines bespoke
   error styling.

2. **`useConfirm()`** is the single way to ask a yes/no question. It's
   a promise-based hook (`const ok = await confirm({...})`) backed by
   Radix's `AlertDialog` primitive. Same async-imperative ergonomics
   as `window.confirm` so call sites don't bloat with state. One
   `<ConfirmDialogProvider>` lives at the app root.

3. **`sonner` for toasts.** Picked over `react-hot-toast` and Radix's
   `react-toast` primitive for: smaller bundle (~3KB gzipped),
   `toast.promise(...)` ergonomics that fit our save flows, and the
   visual vocabulary you'd expect (same author as Vaul, used widely in
   Next.js apps). Toaster mounted at the root with `richColors
   closeButton` defaults.

4. **Toasts confirm outcomes outside the form. Errors stay inside.**
   The motivation: toasts disappear; validation messages must persist
   anchored to the field that failed. So:
   - Async success → toast.success
   - Long latency → toast.promise
   - Field-level error → `<FieldError>`
   - Form-level error (network / 500) → inline alert banner in the form
   Never substitute one for another.

5. **HTML form-validation attributes are banned** when there's a JS
   submit handler. Mixing them with our own error UI is the root
   cause of issue (1) above. Single narrow exception: a pure HTML
   form with no JS handler (currently only `InquiryModal`).

6. **Modal close-on-save is the default.** Multi-step flows
   (create-then-add-children) handle the follow-up explicitly by
   reopening in the new mode, not by suppressing the close.

**Enforcement:** ESLint `no-restricted-globals` + `no-restricted-properties`
rules ban `confirm` / `alert` / `prompt` (both bare and `window.*`
forms). The HTML-attr rule is *not* automated — easy to write a
brittle AST selector that false-positives on `<input type="range">`;
catching it at review is cheaper. Pre-commit hook (lefthook) runs
ESLint already, so the bans gate every push.

**Reversibility:**
- Components (FieldError, useConfirm) — reversible. Imports rewriteable.
- sonner — reversible. `toast.*` call sites are few, and migrating to
  another library means a search/replace + a Toaster swap.
- ESLint bans — reversible. Already enforced at the rule level, not
  via codemod.
- Docs (`docs/ui-patterns.md`) — reversible.

**Alternatives considered:**
- *Props-driven `<ConfirmDialog>` component instead of a hook.* Rejected:
  every consumer would need its own `useState` + pending-action state
  just to ask a yes/no. The hook hides that plumbing.
- *Radix `react-toast` primitive over sonner.* Rejected: significant
  styling + queue-management work for a feature sonner gives us with
  a single import.
- *`react-hot-toast`.* Rejected: API is older, no built-in
  `toast.promise` shape, larger bundle.
- *Custom-roll a confirm dialog over Radix `Dialog` rather than
  `AlertDialog`.* Rejected: `AlertDialog` is the purpose-built
  primitive (correct ARIA, correct focus trap, correct keyboard
  semantics). Don't rebuild it.
- *AST-level ESLint rule banning `min`/`max`/`required`/`pattern` on
  inputs.* Rejected: brittle (false-positives on `<input type="range">`,
  `<input type="date">`); review + the doc are sufficient.

---

## 2026-06-22 — T-070: dimensions stay optional at every status, cm-only input, 3 size bands, single band per query

**Context:** The platform schema already has `artworks.dimensions jsonb`,
but nothing in the studio UI exposes it — most published works land
without dimensions, and the planned T-062 size filter has no data to
filter on. Three sub-decisions needed.

**Decided:**

1. **Dimensions are optional at every artwork status.** Drafts can be
   half-filled; publish doesn't gate. A NULL-dimensioned work simply
   never appears in a size-filtered query — silent exclusion.
   - A non-blocking soft-confirm fires on the studio-side
     transition to `published` when dimensions are missing: "You
     haven't added dimensions. Buyers won't be able to filter your
     work by size. Publish anyway?" Two buttons, ship-able from a
     `window.confirm()` in v1.

2. **cm-only input + storage.** No inches toggle. The stored shape
   stays `{"unit": "cm", "width": …, "height": …, "depth"?: …}` —
   `unit` is normalised onto the output even when the input omits it,
   so a future inches-input mode can write `unit: "cm"` after
   conversion without a schema change.

3. **Filter: 3 size bands, single band per query, longest side.**
   - Bands: **S** ≤ 40cm, **M** 41–100cm, **L** > 100cm (longest of
     width + height, depth excluded).
   - URL: `?size=s|m|l`. Unknown values fall through with no clause
     (tolerant — bookmarked `?size=xl` survives a future rename).
   - Multi-select (`size=s,m`) + custom range deferred.

**Alternatives considered:**

- **Require dimensions at publish.** Rejected: would block legitimate
  use cases (digital-only works, conceptual / performance, artists
  testing the studio without the info to hand). Discovery platform
  framing — not inventory enforcement.
- **5 bands (XS / S / M / L / XL).** Rejected for v1: finer
  granularity is meaningless at 2000 seeded rows and pre-real-artist
  scale. Easy to expand later.
- **Multi-select bands in a single query.** Rejected for v1 — same
  shape change we'd want to make to `medium=` and `availability=` at
  the same time, so bundle later.
- **Server-required width AND height when dimensions is set.**
  Accepted (this IS the validator's rule). Half-filled rows would
  silently fall out of every size-filtered query — confusing for the
  artist who entered partial data, and harder to debug.
- **Inches input toggle now.** Deferred — adds a validation surface
  and a conversion bug, and most non-US contemporary art uses cm
  natively. Revisit when an artist asks.

**Why:** The friction calculus changed when I (the operator) hit it
myself during the first real-artist test of the studio. An artist who
hasn't measured their work shouldn't be blocked from publishing — but
buyers do want a size filter that works, which the soft nudge nudges
toward. Three bands is the smallest set that's meaningfully useful.
cm-only keeps the input + storage + filter all in one unit so we
don't accumulate conversion bugs.

**Reversibility:**

- Optional-at-publish: **High.** Add a publish-gate check in the patch
  handler (matches existing status check pattern).
- cm-only: **High.** Adding inches input is a UI-only change with
  conversion at write-time; storage shape already accommodates it.
- 3 bands → 5 bands: **High.** Add two more `SizeBand` entries
  client-side + two more match arms server-side; existing data and
  URL tokens carry through.

---

## 2026-06-22 — Demote 5 WAF body-content sub-rules to COUNT for binary-upload + JSON-webhook paths

**Context:** AWS WAF Managed Rule `AWSManagedRulesCommonRuleSet` ships
with five sub-rules that inspect the first 8KB of every POST body:

- `SizeRestrictions_BODY` — blocks any body over 8KB (any real image)
- `CrossSiteScripting_BODY` — XSS heuristics
- `GenericLFI_BODY` / `GenericRFI_BODY` — local / remote file-inclusion patterns
- `EC2MetaDataSSRF_BODY` — 169.254.169.254-like patterns

Two prod traffic shapes hit all five as false positives:

1. **Image uploads.** Adobe XMP metadata in PNG/JPEG from Photoshop /
   Lightroom / Affinity reads as `<x:xmpmeta xmlns:x="adobe:ns:meta/">`
   — the XSS regex matches `xmlns:x` + `adobe:ns:meta/` immediately.
   Random binary bytes in image data occasionally trip LFI/RFI/SSRF
   patterns too. Confirmed via WAF log capture during the 2026-06-22
   cutover of T-054.
2. **JSON webhook bodies.** Cloudflare Email Worker → api webhook
   POSTs are tiny (~250 bytes) but still POSTs — `NoUserAgent_HEADER`
   was the actual blocker here, not body-content, but the body rules
   would also fire on multipart payloads from any future webhook.

**Decided:** Add `rule_action_override` entries in both web + api WAF
ACL configs that demote all five sub-rules to `count {}`. They still
match (visible in metrics + sampled requests + WAF logs); they just
don't terminate.

**Alternatives considered:**

- **Scope-down statements** restricting the managed rule group to
  specific paths (everywhere EXCEPT `/studio*`, `/onboarding*`,
  `/v1/uploads/image`, `/v1/webhooks/*`). Rejected: path-list grows
  with every form, and the WAF still sees the bodies it can't
  protect anyway.
- **Raise the body-inspection size limit** (`AssociationConfig.RequestBody.CLOUDFRONT.DefaultSizeInspectionLimit`)
  to 32KB/64KB. Rejected: only addresses size, not content false
  positives; costs extra per CF request.
- **Disable the whole Common Rule Set.** Rejected: header / URI / cookie
  sub-rules in CRS still catch real attacks (request smuggling,
  oversized cookies, restricted file extensions) and have zero false
  positives on our traffic.

**Why:** Image-upload routes can't be meaningfully protected by WAF
body inspection — binary data inherently contains patterns that match
any text-oriented heuristic. The real guards for upload routes live at
the app layer (mime sniff, dimensions, Rekognition moderation T-008,
size limits). WAF body inspection is a strong second line of defence
for *form*-shaped POSTs (login, comment, etc) but the wrong tool for
binary uploads. Surfacing the rule as COUNT keeps observability without
breaking the user-visible flow.

**Reversibility:** **High.** Each override is a 5-line Terraform block
in `modules/{web,api}/main.tf`. Removing them re-enables BLOCK on the
next apply. The override list is per-rule, so we can re-enable any
single rule individually if its false-positive rate drops (e.g. if a
future Adobe-XMP-aware WAF update lands).

**Knock-on:** WAF logging now flows to `aws-waf-logs-ml-art-prod-{web,api}`
log groups (also adopted into TF in this commit). 3-day retention,
triage-only. Watch the COUNT metric in CloudWatch; if a real attacker
trips one of these and gets through, we'll see the count without the
404 telemetry being silent.

---

## 2026-06-20 — Server-side `anon_pending_actions` table for queued anon intents (over cookie storage)

**Context:** `T-052c` captures intents from signed-out users (clicking Follow without an account) so the merge-anonymous handler can replay them after sign-in — closing the "lose the highest-intent click in the funnel" leak. Two storage options were live:

- **(a) Extend the anon-id cookie** with a `pending_follows: [uuid, ...]` field. Cookie size cap ~4KB → ~80 entries. No DB table.
- **(b) Server-side `anon_pending_actions (anon_id, kind, payload, expires_at)` table** with a single endpoint to insert.
- **(c) Browser localStorage** for client-only capture.

**Decided:** (b). Single endpoint `POST /v1/anon/pending/follows/:artist_id`; the merge handler drains rows keyed on the anon-id cookie post-sign-in.

**Alternatives considered + rejected:**
- **(a) Cookie storage** — workable but the cookie grows with every queued intent and gets sent on every request thereafter (bandwidth tax). Mutating cookies cleanly from client code requires same-domain JS. Generalising to richer future intents (save-to-collection-X, queue inquiry to Y) means cramming larger payloads into a header on every request.
- **(c) localStorage** — simplest client-side but doesn't survive private mode or device switch, and can't be replayed by the server-side merge handler — only by client code, which means yet another moving piece in the post-sign-in dance.

**Why (b):** generalises beyond Follow (save-to-collection has exactly the same anon-click-lost problem); keeps the cookie minimal (it stays just the signed UUID); auditable + queryable in psql when debugging; the merge handler already runs in a transaction that touches `uploads` + `events` — adding `anon_pending_actions` to that transaction is a few more SQL statements, no new orchestration. Schema is intentionally generic (`kind text, payload jsonb`) so future intents add a `kind` value + a match arm in the merge handler, not a new table.

**Reversibility:** High. If the table grows uncomfortably big we can add a cleanup cron (`expires_at < now()` delete) or layer a cookie hint as a fast path. If we ever want to switch to cookie storage entirely the API surface stays the same shape — POST to record, the merge handler drains.

---

## 2026-06-19 — Pin Lambda's view of `Host` via API Gateway parameter mapping (not custom domain, not middleware)

**Context:** Clerk's middleware rejected our session-handshake redirects with `redirect_url is invalid` because the Lambda was seeing `Host: <apigw-invoke-url>.execute-api.…amazonaws.com` instead of `wander.gallery`. CloudFront's `AllViewerExceptHostHeader` origin policy strips the original Host for SNI compatibility with API Gateway's cert. Three architecturally distinct fixes were on the table:

- **(a) API Gateway custom domain** for the origin, with DNS shenanigans (CNAMEs, multiple `aws_apigatewayv2_domain_name` rows, dual-cert routing) so the Lambda sees `Host: wander.gallery` natively. Cleanest from a "what the Lambda sees matches reality" angle.
- **(b) Middleware-layer request reconstruction** (rebuild the `NextRequest` with the right URL + Host before clerkMiddleware sees it). Simplest in TF terms.
- **(c) API Gateway HTTP API parameter mapping** to rewrite headers at the integration → Lambda boundary: `request_parameters = { "overwrite:header.Host" = "wander.gallery" }`.

**Decided:** (c). Three lines of Terraform, no DNS work, no custom domain, no middleware reconstruction. The Lambda receives a payload-format-2.0 event whose `headers.host` is the rewritten canonical value; OpenNext converts that into a NextRequest whose URL reflects `wander.gallery`. Every downstream consumer — Clerk's middleware, `headers().get('host')`, request-derived URL helpers — sees the right hostname natively.

**Alternatives considered + rejected:**
- **(a) Custom domain** — the right answer in a textbook setup, but our CloudFront-fronted-API-Gateway topology with `AllViewerExceptHostHeader` for SNI made the cert + DNS choreography surprisingly tangled (CloudFront resolves origin via `origin_domain_name`, SNI is tied to that, can't differ from the Host CF sends without an extra DNS layer + an extra ACM cert). Real work for marginal upside.
- **(b) Middleware reconstruction** — tried in three forms (streaming body forwarding, ArrayBuffer body buffering, header-only synthesis) and every variant hung the Lambda at the 10s timeout. clerkMiddleware apparently does an outbound call when it sees a "canonical" host that never returned in this environment. A 5-line change that worked locally and bricked prod twice; vetoed by experience.

**Why (c):** smallest-surface fix that solves the actual problem; sits at the natural seam between API Gateway and Lambda; the rewritten `Host` is visible to every downstream layer including the OpenNext bundle without code changes; no DNS, no certs, no extra resources. Documented inline at `infra/modules/web/main.tf` so the next person doesn't re-discover the cert / SNI dance by accident.

**Adjacent fix from the same debugging session:** the middleware now 308-redirects direct hits to the API Gateway invoke URL back to `wander.gallery` (URL-healing for stale bookmarks + autocomplete entries from earlier testing). Detection is via `X-Amz-Cf-Id` absence — we can't use the now-rewritten Host. The full lockdown of the API Gateway URL behind a CloudFront shared-secret header is tracked separately as `T-064`.

**Reversibility:** High. Removing the `request_parameters` line reverts to the broken behaviour; the rest of the stack stays intact.

---

## 2026-06-17 — Event storage: Postgres hot tier, S3 Parquet cold archive (when volume justifies)

**Context:** `T-050` introduces a write path into the existing `events` table. At realistic mid-term scale (~1k DAU, 10-20 events/session) that's 10-100K events/day → 20-35M rows/year → ~10-15 GB/year on Neon. Storage cost is real ($0.30/GB-month); query latency on analytical aggregations gets painful past ~100M rows in a single non-partitioned table. Question: where do events live in the medium term?

**Decided:** Postgres now, S3 Parquet cold archive later — driven by volume, not in advance.

- **Now (`T-050` + promoted `T-016`):** events write to Postgres `events`, partitioned monthly. The ML jobs (`user_profile.refresh`, recommendation, Discover Weekly) read recent partitions only — they weight by recency and don't care about >60-day-old data.
- **Later (~6 months of real usage, or when partition row count hits ~10M):** nightly Lambda exports yesterday's partition to `s3://wander-events/year=YYYY/month=MM/day=DD.parquet`. Both tiers populated from this point.
- **Even later (~12 months in, or when query latency hurts):** drop Postgres partitions older than 90 days. Athena or DuckDB over the S3 archive picks up long-tail analytics.

**Architecture commitment:** events are written via `JobEvent::EventLog` (handler-side fire-and-forget), never in the request transaction. This decouples storage from the request path and means the future storage swap is purely a handler implementation change.

**Alternatives considered + rejected:**
- **Pure data lake from day one** (S3 + Firehose). Rejected at v1 volume: eventual consistency complicates the ML read path (taste-vector job either reads via Athena at 5-30s latency or maintains a Parquet → Postgres extract); operational complexity isn't justified by 10-100K events/day.
- **Clickhouse / Timescale / specialised OLAP.** Rejected: new service to operate, new credentials to rotate, new failure modes. Worth revisiting only if a real-time analytics use case emerges that Athena over Parquet can't serve.
- **Keep everything in Postgres forever.** Rejected: storage cost on Neon at 100M+ rows is real, and analytical query latency gets painful even with partitioning. The S3 archive layer is cheap insurance.

**Why:** Postgres + monthly partitioning carries us cleanly through v1.x scale with zero new infra. The job-queue write abstraction means future storage tier additions are additive, not migrative. We don't pay for a data lake until we get the value of one.

**Reversibility:** Medium. The `JobEvent::EventLog` abstraction is the swap point — changing the storage destination is a handler-impl change, not a schema change. But once analytics queries depend on a particular storage shape (Postgres SQL vs Athena SQL), they're somewhat coupled.

---

## 2026-06-17 — Algorithmic neighbourhoods as primary discovery primitive

**Context:** Hand-curated neighbourhoods shipped with v1 (6-12 hand-picked themes from `seed.py::_NEIGHBORHOODS`). `99-deferred.md` originally called for algorithmic clustering (HDBSCAN + LLM label) only "when corpus > 2000 artworks." With v1 sitting at ~2000 demo artworks and real artists arriving, the threshold question is becoming live. Compounded by the editorial decision (below) which removes hand-curation as a sustainable surface.

**Decided:** Promote algorithmic neighbourhoods to the primary discovery primitive (`T-057`). The schema already supports it — `neighborhoods.kind ∈ ('curated', 'semantic', 'geographic')`, `cluster_centroid vector(1024)`, `representative_artwork_ids uuid[]`, `computed_at timestamptz` were all designed for this. Algorithmic clusters power neighbourhood pages, cluster-of-an-artwork ("more from this neighbourhood"), cluster-of-a-user (sort neighbourhoods by sim-to-taste), and feed sampling for Discover Weekly.

**Alternatives considered + rejected:**
- **Keep hand-curated; defer algorithmic until 10k+ artworks.** Rejected — hand-curation doesn't scale on founder time (see editorial decision below), and at the current corpus HDBSCAN already produces meaningful clusters even if they reflect WikiArt's style structure today.
- **Pure personalised feed; no clusters.** Rejected — clusters are the *public* navigation surface (everyone shares the same neighbourhood URLs, can link to them, can browse them anonymously). Personalised feeds can't substitute for a shared vocabulary of regions.

**Why:** The schema was clearly designed for this; using only the `curated` kind today is leaving most of the table empty. Algorithmic clusters compose with every other ML feature (`T-055` taste vector, `T-060` Discover Weekly, `T-061` calibrator). Wider corpus surface area is more navigable when the regions of the embedding space have stable named addresses (`/neighborhoods/moody-coastal`) that update their membership weekly but keep their slugs.

**Coexistence with `kind='curated'`:** the hand-curated set stays in place during the transition. UX call (filter? merge? curated-first then algorithmic?) deferred to `T-057` implementation. The schema supports either.

**Reversibility:** High. Disabling the algorithmic job leaves the `'semantic'` rows untouched but stale; falling back to curated-only is a `WHERE kind = 'curated'` clause.

---

## 2026-06-17 — Discovery is ML-driven; no editorial curation surface

**Context:** The discovery layer has two paradigms available: founder-led editorial picks (hand-curated neighbourhoods, weekly "what we're looking at" digest, taste-as-brand voice) or ML-driven retrieval (per-user taste vectors, algorithmic neighbourhoods, content-based recommendations). The strategy review surfaced this as a forking-decision point because both paths shape the same user-facing surfaces — homepage row, neighbourhood concept, weekly digest — but require different ongoing investment and yield different products.

**Decided:** ML-driven. All retention-loop surfaces (homepage rows, neighbourhood pages, weekly digest, "for you" feed) are computed from event-driven taste vectors and HDBSCAN-discovered clusters. There is no editor-led "curator picks" surface, no founder-as-taste-voice positioning, no manual weekly newsletter.

**Alternatives considered + rejected:**
- **Editorial-led** (curator picks + weekly newsletter + hand-picked neighbourhoods). Rejected: doesn't scale on founder time; collapses the moment the founder stops curating; ties brand identity to one person's taste rather than to the product's aesthetic + the quality of the artists on it.
- **Hybrid** (algorithmic primary + editorial accent row). Rejected on net — the editorial accent always becomes the visual centre of gravity because it's higher contrast (a person picked these). Pulls energy from the ML loop without adding much. Revisit only at scale (10k+ artists).
- **Personalised-only, no clustering.** Rejected: cold-start is brutal without clusters to navigate. Algorithmic neighbourhoods provide the public navigation primitive (everyone sees the same clusters) that personalised feeds can't.

**Why:** Aligns with positioning — "no marketplace, no commission, no human-driven ranking" generalises to "no editor-driven ranking either." ML produces fresher surfaces with less ongoing work, scales naturally as the corpus grows, and composes with every other ML feature (search re-rank, similar-artists, Discover Weekly). Brand voice is communicated by aesthetic restraint and the quality of artists on the platform, not by editorial picks.

**Reversibility:** Medium. Building an editorial workflow later is straightforward (CMS + a row component). Tearing one out once readers depend on it is harder. Sequence is therefore: ship the ML loop first, only consider editorial if it materially under-delivers and we can articulate what an editor would add.

---

## 2026-06-17 — No in-platform messaging; inquiries are email-stitched threads

**Context:** Phase 4b (`T-011`) shipped artist → inquirer reply via Resend; the inquirer can't reply back today. Two paths to close the loop: build a real in-platform messaging UI (real-time chat, notifications inbox), or stitch the conversation by tokenised Reply-To addresses so both parties keep emailing while the platform persists the full thread.

**Decided:** Email-stitching, indefinitely. Conversations are real-time email; we persist the full thread in `inquiry_replies` (extended with `from_role` per `T-054`); both parties see what they expect — artist sees the thread in their studio inbox, inquirer just keeps emailing.

**Alternatives considered + rejected:**
- **Full in-platform messaging.** Rejected: forces the artist to check yet another inbox (hostile to their actual workflow — most live in email), and forces the anonymous inquirer to sign up to reply (destroys 70-90% of conversions). The "feels like an app" upside is mostly cosmetic at our use-case scale.
- **In-platform for signed-in inquirers, email-stitching for anon.** Rejected: bifurcated model that doubles UX and complicates the persistence story.
- **Status quo (no inquirer-back reply at all).** Rejected: real inquiries are conversations; today we silently drop the inquirer's second email into the void.

**Why:** Email is what the parties already use. It handles attachments, mobile push, formatting, search, and 30 years of accumulated UX for free. Our DB still ends up with the full conversation. We graduate to in-platform messaging only when we need a feature email literally can't do — group conversations, structured offer/accept flows, embedded transactions. None of those are v1 or v1.x.

**Reversibility:** High. Same `inquiry_replies` substrate supports both surfaces; adding a UI later is purely additive — no schema change needed.

---

## 2026-05-29 — Jobs queue: Postgres local, SQS + Lambda prod

**Context:** Several v1 surfaces need background work — geocoding `artist_locations` rows (currently `tokio::spawn`, fragile across api restarts), email delivery via Resend (T-032), image moderation via Rekognition (T-008), and the deferred LLM-assisted onboarding (T-012 Phase 2). The state-of-the-build review surfaced the worker-runtime question as the biggest unblocker for those.

The pragmatic options:
- **Inngest** — 50k step-runs/mo free, excellent step-function model, but no first-class Rust SDK. Handler code would have to live in TypeScript with calls back into the Rust API, doubling the deployable surface.
- **AWS SQS + Lambda** — fits the existing `04-stack-and-infra.md` AWS targeting. cargo-lambda support is mature. Free at v0 scale (1M Lambda invocations + 1M SQS messages/mo).
- **Cloudflare Queues** — JS/WASM-centric; awkward for Rust.
- **Postgres-backed jobs table** — self-contained, zero new infra, slow at high QPS but fine at v1 volume.

**Decided:** Same handler code, two drivers.
- **Local dev**: Postgres `jobs` table (migration `0012_jobs.sql`) + a sibling Rust binary (`api/crates/jobs-worker`) that polls with `FOR UPDATE SKIP LOCKED`. Zero external dependencies; runs in the same `make dev` loop as the api.
- **Prod**: SQS queue + cargo-lambda binary triggered on receive. Same `core::jobs::handle` dispatch function runs in both environments.

The abstraction lives in `core::jobs::JobsBackend` — an enum of `Postgres` (today) and `Sqs` (deferred until we deploy), matching the `ObjectStore` / `GeocodingClient` pattern. `JobEvent` is the tagged-enum wire format; the same JSON shape serializes into both a `jobs.payload` jsonb column and an SQS message body. Handlers (`core::geocoding::geocode_and_update` today; future `core::emails::*`, `core::moderation::*`) take a `JobsDeps` struct and return `Result<()>` — no driver knowledge in the handler.

**Alternatives considered + rejected:**
- **Inngest** — rejected: no Rust SDK means writing handlers in TS, doubling auth + config + deployment. The 50k-runs-free is generous but doesn't pay back the bilingual cost for our Rust-heavy backend. Revisit only if T-012 Phase 2 (LLM extract + scrape) lands as TS for other reasons.
- **Keep `tokio::spawn`** — rejected: works for geocoding (where re-saves are cheap) but won't extend to email (where a lost message is a missed inquiry) or moderation (where a lost message is unmoderated content reaching the public surface).
- **One backend now, port later** — rejected for the same reason we picked the enum: we know we'll need a different driver in prod, so the abstraction has to exist from day one. Otherwise every handler call site bakes in the local assumption.

**Why:** Pays back across every future background job. Each new job is one `JobEvent` variant + one handler fn + one match arm in `handle()` — local + prod both just work. The migration to SQS+Lambda when we deploy is a new ~50-line binary + an env flag, not a rewrite.

**Reversibility:** Medium — `core::jobs` is the central abstraction; rewriting it would touch every job-enqueuing site. But the on-the-wire format is plain JSON, so adopting a different orchestrator (Inngest, Trigger.dev, Hatchet) later is just a new driver impl — handler code stays put.

---

## 2026-05-29 — Map-search filter semantics: per-artist, not per-artwork

**Context:** `/search?q=ukiyo&map=1` could plausibly mean two things: (a) "venues whose artist has *any* artwork matching ukiyo" (per-artist), or (b) "venues with at least one artwork matching ukiyo on display right now" (per-artwork, stricter). The data layer can express either — the EXISTS subquery on `artworks` is a one-line change.

**Decided:** Per-artist match. A venue surfaces if the artist who lists it has any matching artwork in their portfolio.

**Alternatives:**
- **Per-artwork match** — stricter, more accurate to "find ukiyo prints near me." But artists list venues at the artist level (an `artist_locations` row says "you can see *me* at Foo Gallery"), not at the artwork level. We don't model "which artworks are at which venue" at all in v1 — that's the deferred shows / events Phase 2 work. Per-artwork match would imply a contract we can't currently honor.
- **Defer the decision** — option C from the original triage. Rejected because the question is binary and shipping required a choice; leaving it ambiguous would have meant inconsistent behavior across keyword vs medium vs artist filters.

**Why:** Matches the model of the data — venues are per-artist, so filtering venues should be per-artist. Lower friction: a viewer searching "ukiyo near me" gets every venue that has an artist working in that style, which is what they actually want for a Saturday gallery crawl. Stricter "this specific painting is at this gallery today" UX needs the post-v1 events model.

**Reversibility:** High — one SQL change in `api-search::search_map`, no schema impact. If real users ask "I went to the gallery and the ukiyo print wasn't there," we revisit.

---

## 2026-05-28 — Geography promoted from post-v1 to v1 (lean slice)

**Context:** During the feature-review pass, the user surfaced geography as a key personal motivator: "I don't find it easy to find local galleries or artists whose work I can go look at in person." The current shape is half a foundation — `artists.city/country/lat/lng` columns, `/v1/search?near_lat&near_lng`, a location filter on FilterBar — but no map UI, no live geocoding job, no street-level locations. `99-deferred.md` carves the full geographic story into three phases; Phase 1 (map view + geo neighborhoods) was deferred largely because it didn't have an internal champion yet.

City-only pins are useless on a map ("the artist is somewhere in Berlin"). Useful pins need a street address, which means a place a viewer can actually go — a gallery the artist is represented by, or an open studio. That's a different entity from the artist's "based in" city.

**Decided:** Promote a lean geography slice to v1 as `T-038`. Specifically:

1. Add `artist_locations` table — one row per place an artist's work can be seen. `kind` is `'gallery' | 'studio'` (shows deferred). Street-level address, geocoded to lat/lng.
2. Mapbox geocoding via an Inngest job. Stubs to no-op when `MAPBOX_TOKEN` is absent, matching the existing degrades-gracefully pattern.
3. Studio settings gets a "Where to see my work" CRUD section. Self-listed, trust-based, with a "Listed by the artist" label on the public pin (no admin verification in v1).
4. Artist profile gets a map widget showing the artist's `artist_locations` pins; falls back to a "based in {city}" pill if none.
5. `/search?map=1` toggles grid → map. Clustered pins are `artist_locations` rows. Bounds in URL so views are shareable.

Explicitly **not** in this slice:
- Shows / events as time-bound entities (still post-v1; needs the `events` table).
- `spaces` as first-class entities with their own pages. Two artists at the same gallery just have duplicated `artist_locations` rows; we eat the denormalization for v1 because the venue page is not the product yet.
- Admin moderation queue for galleries. The "Listed by the artist" label is the trust model; a 'Report listing' link can come later if abuse appears.
- Geographic neighborhoods (`neighborhoods.kind = 'geographic'`). Still post-v1 — editorial work, not a code path.

**Alternatives:**
- **Stay deferred, ship v1 without maps** — fastest. But the user has explicitly named this as a differentiator they care about; shipping v1 without it means relaunching the surface later.
- **Full Phase 2 (`spaces` + `events` tables, claim flows, admin moderation)** — the "right" model long-term, but several weeks of build and a moderation problem we're not ready to own. Deferred.
- **"Just-cities" pins** — what 99-deferred's Phase 1 had. Rejected: city-level pins don't drive in-person discovery, which is the whole point.
- **Google Maps embed (user's first instinct)** — Mapbox already has a token slot in env config and is in `04-stack-and-infra.md`'s cost model. Mapbox GL JS is open-source-licensed, supports vector tiles + clustering natively, and the free tier (50k monthly map loads) is generous for v0 traffic.

**Why:** The intermediate "artist_locations as a JSON column on artists" was tempting (no new table). Rejected because we need to query pins by bbox for the `/search?map=1` map mode, and a JSON-blob filter is harder to index than a (lat, lng) on a normalized row. The shape we're picking is forward-compatible with Phase 2 — when we eventually add `spaces`, we migrate `artist_locations` rows into `space_artists` join rows; nothing thrown away.

Cost impact: Mapbox geocoding is free up to 100k requests/month, well above any v0 traffic. Map loads via GL JS: 50k/month free. Both line up with existing `COST.md` guardrails.

**Reversibility:** Medium — `artist_locations` is one table + one Inngest job + two UI surfaces. If we decide to consolidate into `spaces` later, migration is mechanical (one row per `artist_locations.id` → `spaces` + `space_artists` join). The studio CRUD surface stays; only the underlying table changes.

---

## 2026-05-27 — Pre-commit hooks via lefthook

**Context:** Today's audit caught silent drift: `cargo fmt --check` fails (`artwork.rs`), `cargo clippy -- -D warnings` fails (`auth.rs`, `models.rs`), `eslint` fails (`SaveModal.tsx`'s `set-state-in-effect`). All four CI workflows enforce these, so either CI is currently red or we've been lucky on toolchain timing. Local development is on the honor system and the honor system has stopped working.

**Decided:** Adopt `lefthook` (https://github.com/evilmartians/lefthook) with a `lefthook.yml` at the repo root. Per-language path filters so a Rust change only triggers `cargo fmt --check` + `cargo clippy`, a web change only triggers `eslint` + `tsc --noEmit`, etc. Same lint set as CI, so anything passing pre-commit will pass CI.

**Alternatives:**
- **husky** — npm-installed, ties the hook system to `web/`'s package.json, weird for Rust-only PRs.
- **pre-commit** (Python framework) — slow startup (~1s+ per run), requires Python; we already touch Rust/TS/Python so adding another framework is friction.
- **Raw `.githooks/`** — no install/version story, hard to share, gets out of sync.
- **CI-only enforcement (status quo)** — push → red → fix → push cycle; expensive for trivial drift. Doesn't help local dev.

**Why:** Single Go binary, no per-language runtime, ~50ms startup, clean glob-based config, language-agnostic. Same maintainers (Evil Martians) keep up with Rust/TS toolchain quirks. Critically: it can also enforce our TODO comment convention via a regex check (see next entry).

**Reversibility:** High — `lefthook.yml` and `lefthook install` are the only artifacts; removing means deleting the file.

---

## 2026-05-27 — TODO comment format: `TODO(T-NNN): description`

**Context:** Grep across the Rust code shows 5 inline `TODO`s. Three of them — including `inquiries.rs:16` (`(TODO T-XXX)`) and `inquiries.rs:191` (`TODO: enqueue Inngest…` no ticket) — have no resolvable ticket reference. They're notes that will rot. `search.rs:109` cites `T-018` but that ticket is about something else, so the link is broken.

**Decided:** Every inline `TODO` in source code must reference a ticket from `TODO.md` in the form `TODO(T-NNN): short description`. `FIXME` and bare `TODO:` are not allowed. Enforced by a regex check in `lefthook.yml` (pre-commit) — the hook scans the staged diff and rejects commits introducing a bare TODO. CI runs the same check as a backstop.

**Alternatives:**
- **Honor system** — what we have now. Doesn't work.
- **Custom clippy lint** — overkill for what's basically a grep; would need a clippy plugin or a separate lint crate.
- **Allow free-form TODOs, archive in CHANGELOG when removed** — loses the "this code knows about an open ticket" signal that helps reviewers cross-check.

**Why:** A ticket-prefixed TODO is greppable (`grep -r 'TODO(T-007)'` lights up every site that depends on it), traceable (the ticket has the why), and removable (when the ticket lands, you `grep` and delete the stragglers). Bare TODOs accumulate forever; the X-X-X placeholders we currently have are proof.

**Reversibility:** High — undo the regex rule, the existing TODOs stay valid.

---

## 2026-05-27 — `User` as an axum `FromRequestParts` extractor

**Context:** Today, 9 handler call sites do `let user = auth::authenticate(&headers, &state.jwt_verifier, &state.pool).await?;` literally. The original `core::auth` module note ("orphan rules for foreign-trait extractors against cross-crate state aren't worth the abstraction cost at this stage") was correct at 1 site, debatable at 4, wrong at 9. T-011 (studio) will roughly double the count.

**Decided:** Add `impl FromRequestParts<Arc<AppState>> for User` in `api-search` (the binary crate that owns `AppState`), delegating to `core::auth::authenticate`. Handlers go from `headers: HeaderMap` + an explicit auth call → `User(user): User` in the signature. The unit-tested function stays in `core`; the extractor is a thin adapter that lives where the orphan rules allow.

**Alternatives:**
- **Keep inline calls** — verbosity scales linearly with the handler count, and forgetting the call is a silent auth bypass.
- **Generic `FromRequestParts<S> where S: HasAuthContext`** in `core` — more flexible (any AppState can implement the trait), but adds an abstraction that doesn't pay rent until we have a second binary. Worth doing when `api-uploads` lands.
- **`axum::middleware::from_fn` that injects a `User` into request extensions** — works but tests have to set extensions manually; the `FromRequestParts` route uses the same `authenticate` function for both runtime and tests.

**Why:** Orphan-rule-friendly placement (extractor lives with `AppState`, function lives with the logic). One concrete impl rather than a trait we don't need yet. Removes ~9 lines of boilerplate now, double that after studio. Auth failures become structurally impossible to forget at the handler level.

**Reversibility:** Medium — once handlers depend on the extractor signature, undoing means touching all of them. But the contract surface (`User { id, clerk_user_id, email, is_admin }`) doesn't change.

---

## 2026-05-27 — Error reporter shim (web) — one function today, Sentry later

**Context:** 9 web call sites use `console.error("...failed", e)`. In Vercel prod, these go to function logs nobody monitors. Observability is on the pre-launch checklist but Sentry-or-equivalent isn't wired and we don't have deploy infra yet anyway.

**Decided:** Introduce `web/src/lib/reportError.ts` exporting `reportError(err: unknown, context?: Record<string, unknown>): void`. Today it wraps `console.error` with a structured prefix (`[err]`) and the context object. When Sentry (or Axiom, or whatever) gets wired, only this file changes. Migrate the 9 existing call sites in the same pass. Going forward, `console.error` is reserved for genuinely-not-an-error logs (debug prints) and is grep-rejectable in code review.

**Alternatives:**
- **Wire Sentry now** — premature; no deploy infra, no traffic, no signal on what to capture.
- **Keep `console.error`, migrate later** — every call site changes twice (once when we standardize prefix/context shape, once when we add Sentry).
- **Class-based logger with levels** — over-engineered. We only have one level that matters (errors) until we have real users.

**Why:** Cheapest possible seam. Zero behavior change today, one-file change when the real reporter lands. The 9 call-site touches happen once, in this pass, while we're already touching the web tree.

**Reversibility:** High — it's a function. Delete the file and swap back to `console.error` if we change our minds.

---

## 2026-05-27 — Specs (`01..03-*.md`) are aspirational, CHANGELOG + decisions are truth

**Context:** `01-page-spec.md`, `02-component-library.md`, `03-api-data-spec.md` were written as the v1 product spec before the build started. Since then we've shipped rate-limit middleware, `contains_artwork`, neighborhood filters, FilterBar, SaveModal a11y, etc. — none of which is reflected back into the specs. Choice: (a) update the specs on every PR (tax, churn, mostly unread), (b) let them drift unbounded (currently happening), (c) reframe them.

**Decided:** Reframe. The specs describe the *intended v1 product* — useful as a holistic reference. `CHANGELOG.md` + `decisions.md` are the source of truth for *what was built and why*. Update specs only when (i) something in the spec materially contradicts shipped behavior or (ii) we're starting a new major surface and need the spec to scope the work. Otherwise, decisions log the deviation and CHANGELOG logs the build. Add a header line to each spec doc making this explicit.

**Alternatives:**
- **Update specs on every PR** — high overhead, low readership, real chance the spec lags anyway.
- **Delete the specs, let CHANGELOG carry everything** — loses the holistic "v1 product brief" that's still useful when scoping new pieces.
- **OpenAPI-generated API spec** — would solve 03-api-data-spec drift, but requires Rust handler annotation plumbing we don't have. Worth revisiting near launch.

**Why:** Matches how the docs have actually been used — read once at scoping time, rarely thereafter. Avoids spec-maintenance churn that nobody benefits from. Keeps the long-form v1 brief intact and useful.

**Reversibility:** High — the docs exist; switching to per-PR maintenance is a process change, not a code change.

---

## 2026-05-27 — Rate limiting lives at the API, not the edge (for now)

**Context:** Standing up rate limiting (`T-007`). Two reasonable places to put it: edge (AWS WAF in front of Lambda, or Vercel middleware), or right at the API.

**Decided:** Implement at the API layer first, in-process via `governor` (GCRA / leaky bucket), keyed per-user → per-anon → per-IP. Limit middleware lives in `core::middleware::rate_limit`. Edge rate limiting is tracked separately as `T-034` (AWS WAF) and `T-035` (Vercel middleware), gated on actual deploy infra.

**Alternatives:**
- AWS WAF rate-based rule in front of the Lambda Function URL. Coarser (per-IP, 5-min window minimum), and we don't have any infra yet so the Terraform would be untested.
- Vercel edge middleware with Vercel KV counters. Doable now since Next.js middleware runs in dev — but only protects traffic that goes through Next.js, and isn't where the actual cost is.
- Tower's built-in `RateLimitLayer`. Global per-process, no per-key state — useless for blocking one abusive caller without blocking everyone.
- Upstash (managed Redis) for distributed limiting. Right answer when we have more than one API process; premature today.

**Why:** The expensive surface is the Jina embedding call behind `/v1/search` and (later) Anthropic / Rekognition behind upload and onboarding jobs — not Lambda invocations themselves. To save $1 in Lambda we'd need to block ~2.5M requests; to save $1 in Jina spend we only need to block ~10k novel queries. Putting the rate limit right next to the paid call is what caps spend. Edge layers add defense-in-depth and they're worth doing — but they go with the deploy milestone, not before there's an edge to put them on.

**Reversibility:** High — the API-layer limiter is one module + a Config flag. Swapping the in-process `governor` for an Upstash-backed implementation is a single trait swap; the middleware contract doesn't change. Adding WAF / Vercel layers later is purely additive.

## 2026-05-24 — Defer the pre-built-portfolio claim flow

**Context:** Original spec had a cold-outreach mechanic where we'd scrape a target artist's website, build a private preview portfolio, and email them a tokenized link to claim or take down.

**Decided:** Remove from v1. Direct manual outreach to 20–30 artists for v0/v1 instead.

**Alternatives:** Build it private-by-default behind a token-gated URL.

**Why:** Even private, republishing scraped work without explicit consent has real legal and reputational risk. Direct outreach is slower but unambiguous. Schema fields for the claim flow are documented in `99-deferred.md` for when we revive it.

**Reversibility:** High — schema is documented, just not migrated.

---

## 2026-05-24 — All-AWS infra over Vercel

**Context:** Two viable hosting strategies — Vercel for frontend + Vercel functions for API, vs all-AWS via OpenNext + Lambda + Terraform.

**Decided:** All-AWS, fully Terraformed. OpenNext for Next.js, Rust Lambdas behind API Gateway, Neon Postgres, S3 + CloudFront, Inngest, Clerk.

**Alternatives:** Vercel + Next.js route handlers (~1 day faster to ship, more vendor lock-in).

**Why:** Single cloud, single IaC story, no cross-cloud secret management. Cost scales more predictably. Marginal extra setup; recovers itself in less iteration friction.

**Reversibility:** Medium — moving to Vercel later means rewriting `infra/` and adjusting OpenNext-specific edges.

---

## 2026-05-24 — Rust Lambdas for the API

**Context:** We could write the API in TypeScript (Next.js route handlers) or Rust (Lambda).

**Decided:** Rust Lambdas, structured as a Cargo workspace, deployed via Terraform.

**Alternatives:** Next.js route handlers (faster iteration, single language with the frontend).

**Why:** User preference + Rust's cold-start performance and type-safe SQL via sqlx. Accepted tradeoff: slower iteration, more boundary work for shared TS types.

**Reversibility:** Low — undoing this means rewriting the entire API.

---

## 2026-05-24 — Local embedder for spikes, HTTP API in production

**Context:** Multimodal embedding can run locally via PyTorch on MPS, or via Jina's HTTP API.

**Decided:** Both, behind the same `Embedder` Protocol. `LocalJinaClipEmbedder` for spikes / batch eval (free, no rate limits). `JinaEmbedder` HTTP client for production request-time embedding.

**Alternatives:** Only HTTP (simpler, pays per spike); only local (impractical in Lambda).

**Why:** Spikes do many embedding calls; HTTP would be slow and expensive. Production runtime can't load a 2GB model into Lambda.

**Reversibility:** High — both implementations exist behind the same Protocol.

---

## 2026-05-25 — Ship modifier delta vectors at α=0.8

**Context:** Visual-search modifier buttons ("moodier", "warmer", etc.). Two competing implementations: precomputed delta vectors added to query embedding, or text-fusion RRF.

**Decided:** Delta vectors at α=0.8 as the production path. Text-fusion retained as a fallback.

**Alternatives:** Text-fusion only (simpler, less to maintain).

**Why:** Round-2 spike on WikiArt (2000 images) showed clean modifier shifts at α=0.8 across all five modifiers, with results staying visually related to the source. Delta is also faster at runtime (one vector add vs two retrieval queries + RRF). See `ml/spikes/2026-05-modifier-deltas/FINDINGS.md`.

**Reversibility:** High — `Embedder` protocol abstracts both approaches.

---

## 2026-05-25 — Synthetic-artist demo seeding

**Context:** Need realistic local-dev data without exposing the platform to copyright issues by using real living artists' work.

**Decided:** Seed from WikiArt (2000 images, 27 styles); create one synthetic artist per style (e.g. "Impressionism Studio (Demo)"); flag every demo row with `is_demo = true`. Production deploys filter `is_demo = false`.

**Alternatives:** Use real artist names from WikiArt (impersonation risk), generate synthetic art (defeats the testing purpose), wait for real artists (blocks engineering).

**Why:** Clear separation between demo content and real artist content. `is_demo` is a single boolean filter at every query boundary.

**Reversibility:** High — a single `DELETE WHERE is_demo = true` wipes all demo content.

---

## 2026-05-25 — Geographic minimal in v1, full Spaces+Events in v2+

**Context:** Original spec had artist `location` as free-text only. The art world is structurally local — galleries, openings, fairs — and that's missing.

**Decided:**
- v1: structured `city`, `country`, `lat`, `lng` on `artists`. `location` + `near_me` filters on `/v1/search`. Mapbox geocoding via Inngest job.
- v2 (deferred): map view, geographic neighborhoods.
- v3 (deferred): "spaces" + "events" as first-class entities. Note the naming — "spaces" not "galleries", to include artist-run / project / fair / pop-up venues native to the indie ecosystem.

**Alternatives:** Defer all geographic to v2 (loses a real product axis).

**Why:** Minimal geographic is half a day of extra work and gives 80% of the "find Berlin artists" value. The full Spaces+Events build is weeks; correct to plan but premature to start.

**Reversibility:** High — Phase 2/3 schemas in `99-deferred.md` are additive.

---

## 2026-05-25 — Cargo workspace: one binary per route group (option B)

**Context:** Three options for the Rust API structure — one Lambda for the whole API, one per route group (~8 binaries), or one per handler.

**Decided:** One binary per route group. Initial groups: `api-search`, `api-me`, `api-collections`, `api-uploads`, `api-inquiries`, `api-studio`, `api-onboarding`, `api-events`.

**Alternatives:** One Lambda for everything (simpler, faster cold start because warm pool covers all routes).

**Why:** Different route groups have different memory/compute profiles (search is embedding-heavy, uploads handle file streams, studio is mostly DB-heavy reads). Independent scaling and deploy granularity helps later. Accepted tradeoff: ~8 deploy targets, slightly more cold-start surface area, more boilerplate.

**Reversibility:** Medium — merging Lambdas later is mechanical; splitting is harder.

---

## 2026-06-07 — Search + map are one surface, viewed two ways

**Context:** `/search` shipped with a `Works` / `Where to see them` toggle — two tabs over the same logical query (artworks + their artist locations). UX kept forcing users to choose between "what does it look like" and "where can I see it," and the two endpoints (grid + map) had drifted in subtle ways (different filter semantics, different result sets) that we patched piecemeal (artist_ids thread-through, q-filtered city pivots, disconnect-explainer banner).

**Decided:** the toggle stays as the affordance, but `?map=1` becomes a **split view** — the grid moves to a scrollable side panel (~40% width on desktop, stacked on mobile) and the map fills the rest. Hover/click syncs in both directions: card-hover emphasises pins, pin-hover scrolls the panel; click on either opens detail in the other. State of truth is a single `highlightedArtistId` lifted to the SearchPage; neither half mutates from a hover that originated in itself.

**Alternatives:**
1. Keep the tabs. Loses the relationship between an artwork and its venue — the disconnect-explainer hack proved we'd be papering over the gap forever.
2. Map-as-default with a tab to grid. Too aggressive — users browsing without a geographic intent want the simpler grid.
3. Full Airbnb (map dominant, list as overlay sheet). Right for travel sites where the map IS the product; wrong here because the artwork visual carries primary value.

**Why:** the split view models the relationship as it actually is — every artwork has an artist, every artist may have a location, the user wants both lenses simultaneously when they're searching geographically. The toggle preserves the lighter "just show me artworks" path for users who don't care where to physically encounter the work.

**Reversibility:** Medium. The split layout is a swap in `/search/page.tsx`'s render path; we keep the grid component and the map component separately usable, so falling back to the tab model is a layout change, not a data-model change.

**Implementation:** four slices (L1–L4) in `TODO.md` `T-045`. L1 (layout shell, no sync) is the smallest releasable unit and the right thing to ship first.

---

## 2026-06-09 — Search resume state belongs in the URL, not sessionStorage

**Context:** Users wanted "leave the search page → come back to the same view" — same page of results loaded, same artwork selected, same map viewport, same scroll. First pass used `sessionStorage` keyed by URL with snapshot/restore on mount. It worked, but the failure modes piled up: silent hydration races, dev hot-reload wiping state, the most common case (no Load More yet) wasn't even covered, and the state was invisible to anyone who didn't write the code.

**Decided:** the URL is the single source of truth for search resume state.
- `?pages=N` (cumulative cursor pagination on the server)
- `?focus=<artwork_id>` (selected artwork; set via replaceState on click, restored on mount)
- `?bbox=…` (already lived in URL)
- Filters already in URL

The `<BackToSearchLink>` component uses `router.back()` when the referrer is our `/search` so the full browser-history entry is reused (including scroll), and falls back to `router.push('/search')` otherwise.

**Alternatives:**
1. **`sessionStorage` snapshot + restore on mount.** What we tried. Brittle: hydration race, dev-mode loss, lost-on-first-page-of-session, un-shareable.
2. **In-memory route cache (Linear pattern).** Better than sessionStorage but still hides state in JS land — bookmark, refresh, share-link all break.
3. **bfcache reliance.** Browser's back/forward cache is great when it works; doesn't apply for refresh + share-link + bookmark, and is fragile (caching is disabled by many third-party scripts).

**Why:** the URL is the only address that lives outside the user's tab — making it the source of truth means refresh, share-link, bookmark, and back-nav all produce the same view by construction, not by careful state plumbing. The cost is N sequential `/v1/search` roundtrips per render for `pages=N`, which is acceptable at v1 scale (capped at 10 pages = ~1.5s p95). Future optimisation: parallelise the chase (cursor is internally an offset; we could compute offsets ourselves) — but the API contract stays opaque, so the option is available without an API change.

**Reversibility:** High. The URL-driven approach is additive — if we ever want to re-layer in-memory caching for snappier load-more, the URL stays as the canonical truth and the cache is just a paint optimisation. Reverting would mean nothing more than ignoring the URL params.

## 2026-05-25 — Postgres-backed text query embedding cache

**Context:** Search endpoints need to embed the user's text query at request time. Jina API takes 100–300ms per call. This dominates search latency.

**Decided:** A `query_embedding_cache` table in Postgres: `(query_text PK, embedding vector, model_name, model_version, last_used_at, hit_count)`. Lookup before calling Jina; insert on miss. TTL 30 days via scheduled cleanup job.

**Alternatives:** Redis (ElastiCache too expensive at v1; Upstash adds another service and rate limits), in-process LRU per Lambda (lost on cold start), no caching (slow + expensive).

**Why:** Zero extra infrastructure, free, fast (one Postgres query), survives Lambda restarts. Common queries amortize to a single embedding API call ever.

**Reversibility:** High — swap to Redis later if needed without changing the cache interface.

---

## 2026-05-25 — Dev-only `/dev/login-as/:slug` route for testing the studio surface

**Context:** Seeded demo artists have no Clerk users. To exercise the artist-studio flows locally we need a way to act as an artist without going through real auth.

**Decided:** A dev-only endpoint `GET /dev/login-as/:artist_slug` that mints a development JWT for the matching seeded artist. Gated by `ML_ART_ENV=dev` — refuses to register the route in staging or prod.

**Alternatives:** Create Clerk users for demo artists during seeding (pollutes the Clerk dev instance, gets confusing).

**Why:** Cleanest separation between auth (real Clerk users) and demo data (seeded artists). One env-flag check at startup, impossible to ship to prod.

**Reversibility:** High — delete the file.

---

## 2026-05-25 — Monorepo with per-directory CI

**Context:** We have `ml/`, `db/`, soon `api/`, `web/`, `infra/`. Single repo or split?

**Decided:** Monorepo. CI runs path-filtered workflows per directory.

**Alternatives:** One repo per service.

**Why:** Cross-directory edits are common in early product (schema change touches `db/`, `api/`, `ml/seed.py`, sometimes `web/`). Single repo means one PR.

**Reversibility:** Medium — splitting a monorepo later is mechanical but loses git history.

---

## 2026-05-26 — Clerk testing helper for E2E (real auth, not a web bypass)

**Context:** Playwright needs to cover signed-in flows (Save modal, Inquire when signed-in, future studio surfaces). The originally-tracked `T-031` proposed a web-side test-mode bypass mirroring the Rust `JwtVerifier::for_tests()` pattern — a cookie set by a dev-only route, read by `apiFetch`, forwarded as `Bearer test-<sub>`.

**Decided:** Use Clerk's official `@clerk/testing` package + their test-email convention. No custom bypass code in the web app at all.

How it works:
- Clerk's dev instance auto-accepts the OTP `424242` for any email matching `*+clerk_test@*` (a documented Clerk feature)
- `@clerk/testing/playwright` exports `clerkSetup()` (per-worker) and `setupClerkTestingToken({ page })` (per-test) which bypass Clerk's Smart CAPTCHA / bot fingerprinting so headless browsers can submit forms
- A Playwright `setup` project signs up a fresh user once per run and saves browser state to `e2e/.auth/user.json`
- A `chromium-authed` project picks that state up via `storageState`; tests in `*signed-in*.spec.ts` run under it

**Why this over a custom bypass:**
- No production code paths exist that bypass auth — the *only* thing different in tests is Clerk's bot-protection token. The auth model is real-Clerk-from-the-browser's-perspective.
- Less surface area to get wrong. A bypass cookie that's gated only by env var is one config-mistake away from prod-leaking; this has no equivalent failure mode.
- Real JWTs verify against real JWKS in our Rust API, exercising the actual production verification code path.

**Cost:** each Playwright run creates a real Clerk user in the dev instance + a row in our `users` table. Mild accumulation. A cleanup script (cron-driven, deleting `*+clerk_test@*` users older than a week) is a future-day chore.

**Reversibility:** High — uses an external library + standard Playwright patterns. If Clerk changes the testing helper API, we adapt.

---

## 2026-05-26 — Test-mode JwtVerifier (explicit constructor, not env-gated)

**Context:** Integration tests for authed endpoints (`/v1/me`, `/v1/me/collections`, signed-in `/v1/artworks/:id/inquiries`) need a way to authenticate without minting real Clerk JWTs. Three options were on the table:

1. **Env-flag bypass** in `authenticate()`: e.g. when `AUTH_DISABLED=true`, trust `X-Test-User-Id` header. Rejected — a misconfigured prod deploy could become "anyone can be anyone".
2. **Mint real Clerk JWTs in tests** via Clerk's backend API. Rejected — tests would need network access to Clerk, real secret key in CI, and we'd be testing Clerk's signing as much as our code.
3. **Explicit test constructor on `JwtVerifier`.** Picked.

**Decided:** `JwtVerifier::for_tests()` returns a verifier with a `test_mode: true` flag. In `verify()`, when that flag is set, accept any token starting with `test-` and return a synthetic `ClerkClaims { sub: token[5..] }`. Tests seed users with known `clerk_user_id` values (e.g. `user_test_alice`) and send `Bearer test-user_test_alice`. The `upsert_user` path hits the existing SELECT branch — no Clerk API call.

**Why explicit-constructor over env-flag:** the bypass requires a *code change* to reach (calling `for_tests()` instead of `new()`). Production code paths in `main.rs` call `new()`. There's no way for an environment variable or config file to flip a prod deploy into bypass mode.

**Limit:** doesn't cover the web side. Playwright can't drive Clerk's hosted sign-in. Signed-in browser flows are not covered by E2E yet. See `T-031` in `TODO.md`.

**Reversibility:** High — switching to real Clerk JWT minting in tests is purely additive; the test-mode path can stay.

---

## 2026-05-26 — Cross-user resource access returns 404, not 403

**Context:** Collections endpoints enforce `WHERE user_id = $auth_user_id` in SQL. When Bob tries to read Alice's collection, we have a choice of two error statuses: 403 (you're authenticated but not allowed) or 404 (no such collection).

**Decided:** 404 for everything cross-user. Same response shape as a missing resource.

**Why:** 403 leaks existence — Bob can infer that Alice has a collection with that UUID, which is information he shouldn't have. 404 is honest from Bob's perspective (the collection doesn't exist *for him*) and consistent with how we'd treat any unknown ID.

**Cost:** marginally worse error messages for the legitimate case where Alice mistypes her own collection ID — she also sees 404 instead of "this exists but you can't see it." Acceptable.

**Reversibility:** High — flipping back is a one-line change per handler.

---

## 2026-05-26 — Anonymous identity: cookie at Next, header to API

**Context:** The API spec calls for a signed first-party `anon_id` cookie. With Next.js on `:3000` and the Rust API on `:9100` in local dev (different origins), cookies don't traverse cleanly without CORS + credentials. In production the routing will likely consolidate (CloudFront fronts both `/` and `/v1/*`), so cookies *will* work natively — but only there.

**Decided:**
- Next.js owns identity: middleware sets a signed `anon_id` cookie (HMAC-SHA256 over UUID v7 with `ANON_COOKIE_SECRET`), HTTP-only, `SameSite=Lax`, 1-year expiry
- Server components verify the signature, then forward the *unsigned* UUID to the Rust API as `X-Anonymous-Id` header
- The Rust API treats `X-Anonymous-Id` as trusted because the only thing that should be reaching the API is Next.js (server-to-server). In production this is enforced by CloudFront / API Gateway routing — the API isn't directly reachable from the browser
- Missing header is fine; many endpoints (search, artist, artwork, neighborhoods) work without identity. Endpoints that need it (rate-limited writes, behavior tracking) require it explicitly via an extractor

**Alternatives considered:**
- Browser-direct cookie to API: needs CORS with credentials, separate `Domain` config, more complex
- Client-supplied unsigned header from anywhere: trivially spoofable; rejected
- JWT for anon identity: overkill — we just want a stable opaque id

**Why:** Solves identity in local dev without CORS headaches; matches the production routing model; lets us add real signature verification on the Rust side later if needed (the cookie is signed by *something*, we just choose to trust the Next.js boundary).

**Reversibility:** Medium — switching to browser-direct cookies later is a CORS config + a one-line change in the Rust extractor.

---

## 2026-05-26 — Tiered test posture: integration > E2E > unit, no components

**Context:** No tests exist beyond `ml/tests/test_vectors.py`. We have a working full stack but no automated way to know if a commit breaks things. We need a test posture matched to a solo-dev side project — leaner than enterprise paranoia, but real enough to gate merges.

**Decided:** Four tiers, built in priority order.
1. **Rust API integration tests** via `#[sqlx::test]` against per-test ephemeral Postgres — biggest signal-per-hour
2. **Playwright E2E golden-path suite** in top-level `e2e/` — ~8 flows covering every navigable surface
3. **Vitest units** for pure functions (`formatPrice`, `toQueryString`, etc.) — cheap correctness gate
4. **CI** via GitHub Actions, per-directory gated on `paths:` filters

**Stubbing:** all paid APIs (Jina, Mapbox, Anthropic, Rekognition, Clerk) have deterministic stubs. Same code paths as graceful-degradation dev mode, which `COST.md` already documents.

**Alternatives considered:**
- React Testing Library component tests — rejected: web is mostly JSON-rendering; E2E covers visible behavior, components without explicit tests are easier to refactor.
- Cypress instead of Playwright — rejected: Playwright is less flaky and TS-native.
- 100% coverage target — rejected: incentivises trivial tests.
- Visual regression — deferred: too much churn at v0.

**Why:** Integration tests catch the contract failures that hurt most (wrong JSON, wrong SQL). E2E catches user-visible failures the unit/integration layers can't see. Skipping component tests is the contrarian call — we keep them out *deliberately* because the cost of maintaining them outweighs their value when the components are mostly thin wrappers around fetched JSON.

**Reversibility:** High. Each tier is independent. Adding component tests later is purely additive.

**Full strategy:** see `TESTING.md`.

---

## 2026-05-25 — Artwork detail: full-page first, modal-overlay deferred

**Context:** Original spec calls for `/artworks/[id]` to open as a modal overlay on top of the previous page, with the URL updating; direct-load shows a full page. Next.js supports this via parallel + intercepting routes (`@modal/(.)artworks/[id]`).

**Decided:** Ship `/artworks/[id]` as a regular full page for v0. Defer the modal-overlay pattern to v1.1 (or later).

**Alternatives:** Build the modal-overlay pattern now.

**Why:** Parallel + intercepting routes are powerful but buggy in non-trivial cases (back button, share, SSR/CSR transitions, scroll restoration). Full-page first means: works on first try, easy to test, easy to crawl. The modal layer is a UX polish item, not a feature. Add when the rest of v1 is solid.

**Reversibility:** High — the page already lives at `/artworks/[id]`. Adding the modal is purely additive (new parallel-route slots beside the existing page).

---

## 2026-05-25 — Local-dev port remappings

**Context:** Default Postgres (5432) and Mailhog SMTP (1025) ports collided with existing local services on dev machines.

**Decided:** Map Postgres to `5433`, Mailhog SMTP to `2025`. MinIO (9000/9001) and Mailhog UI (8025) stay default.

**Alternatives:** Use the defaults and assume no conflicts.

**Why:** Conflicts are common on developer machines (local Postgres install, AirPlay on 1025). Non-standard ports documented in `docker-compose.dev.yml` and `decisions.md`.

**Reversibility:** High — change the ports back if the user prefers.

---

## 2026-06-10 — Cloudflare for DNS (forced by Cloudflare Registrar)

**Context:** Domain `wander.gallery` registered with Cloudflare Registrar. Initial TF design used Route53 for DNS (one zone + 3 ACM cert validations + 6 alias A/AAAA records to CloudFront). After bootstrap, discovered Cloudflare Registrar *mandates* Cloudflare nameservers — you cannot point NS at Route53. Transfers are locked for 60 days post-registration (ICANN rule), so we can't relocate the registrar tonight.

**Decided:** Move all DNS records into Cloudflare via the `cloudflare/cloudflare` TF provider. ACM certs stay in AWS (us-east-1) — only the DNS records change provider. Use Cloudflare CNAME-flattening at the apex (CloudFront's `domain_name` as a CNAME, even at `wander.gallery`). All records `proxied = false` so traffic goes direct to CloudFront, not through Cloudflare's CDN (no double-cache, no double-bill, no WAF confusion).

**Alternatives:**
- Transfer to Namecheap / Route53 after the 60-day lock — viable later, not tonight.
- Buy a new domain at AWS Route53 — wasteful.
- Use Cloudflare DNS by hand without TF provider — quick but creates drift.

**Why:** Cloudflare DNS is free, fast, and the TF provider is mature. CNAME-flattening at the apex is the one thing Route53 doesn't do natively (it has alias records, which serve the same purpose). For our shape — three subdomains pointing at CloudFront — Cloudflare DNS is a clean fit.

**Reversibility:** Medium — if we transfer the registrar to AWS later, we'd switch to Route53 (or just leave DNS at Cloudflare and only move the registrar).

**Operational note:** The `CLOUDFLARE_API_TOKEN` env var is required for `terraform plan/apply`. Token needs `Zone:Read` + `DNS:Edit` on the single zone only. Out-of-band rotation hygiene applies.

---

## 2026-06-10 — API Gateway HTTP API over Lambda Function URL

**Context:** Initial deploy used Lambda Function URLs as CloudFront origins — the lighter / cheaper alternative to API Gateway, recommended by AWS for our exact shape. After apply, **every request 403'd** with `Forbidden. For troubleshooting Function URL authorization issues...`, regardless of:
- `auth_type = NONE` + explicit `Principal: *` resource policy ✗
- `auth_type = AWS_IAM` + CloudFront Origin Access Control (OAC) with SigV4 signing ✗
- Custom origin-request policy excluding the `Authorization` header (the AWS-documented OAC collision workaround) ✗

CloudWatch logs confirmed the requests were rejected at the Function URL gateway, *before* reaching the Lambda. Direct `aws lambda invoke` worked perfectly — the function itself was healthy. The account joined the org on 2026-06-09 (one day before this debug session); the most plausible explanation is an **undocumented new-account anti-abuse restriction** that blocks public Function URLs in the account's first few days. No visible SCP / RCP / account-level setting documents this.

**Decided:** Pivot to AWS API Gateway HTTP API (v2) in front of each Lambda. CloudFront → APIG → Lambda. WAF stays attached to CloudFront. APIG was the topology originally expected; the Function URL detour was driven by the "smaller infra, no APIG bill" argument that turned out to be moot for a new account.

**Alternatives:**
- Open AWS support ticket to lift the Function URL block — possible, but days of latency, no guarantee.
- Wait a few days and retry Function URLs — possible but unverified.
- Keep debugging — already 30+ min in with no signal, low expected value.

**Why:** APIG HTTP API is well-trodden, observable, and doesn't have the new-account restriction. Cost is negligible at v1 (`$1.00/M requests`, free tier covers idle). Topology is what we'd build anyway if optimizing for "least surprise per dependency."

**Trade-offs accepted:**
- One extra hop (CloudFront → APIG → Lambda) — adds ~10-30ms p50.
- ~$0–1/mo at v1 traffic vs Function URL's $0.
- 30s hard cap on responses (vs Function URL's 15min) — fine for our workload (SSR p99 ~1s; search ~1s).

**Reversibility:** High — if AWS removes the new-account restriction we can swap APIG back out for Function URL + OAC with the same TF shape as before (the OAC iteration is in git history).


# Art Discovery Platform — Page Spec (v1)

> **Aspirational.** Describes the intended v1 product, not necessarily
> what's in the code today. Truth for shipped behavior lives in
> [`CHANGELOG.md`](CHANGELOG.md); rationale for deviations lives in
> [`decisions.md`](decisions.md). See `decisions.md` 2026-05-27 — Specs
> are aspirational, CHANGELOG + decisions are truth.

## Design principles

- Minimal, white-space heavy aesthetic. Gallery-like, not store-like.
- Server-rendered public pages (SEO). Client-rendered authenticated surfaces.
- URL-driven state for anything shareable (modals, filters, collections).
- Anonymous browsing everywhere. Auth prompt only at save/inquire moments.
- Consistent patterns: one modal style, one card, one grid, one toast.
- Defer features aggressively. V1 proves the discovery experience.

## Global layout

- Persistent top nav (sticky): logo (left), search bar (center, always present), Collections (right, icon when signed in, hidden when not), Profile/Sign-in (right).
- Footer: minimal — About, For Artists, Privacy, Terms.
- No sidebar globally. Filters on search pages live in a collapsible panel above the results grid.
- Light mode only for v1. Dark mode deferred.
- Responsive breakpoints: mobile (<640px), tablet (640–1024px), desktop (>1024px).

## URL structure

| Path | Purpose |
|---|---|
| `/` | Homepage |
| `/search?q=...&filters=...` | Search results (text) |
| `/search?image=<upload_id>&modifiers=...` | Visual search results |
| `/search?map=1&bbox=...&artist=...` | Map mode — venues matching the active filters, with city-pivot pills + "Near me" affordance |
| `/neighborhoods` | Index of all semantic neighborhoods |
| `/neighborhoods/[slug]` | Single neighborhood view |
| `/artists/[slug]` | Artist portfolio |
| `/artworks/[id]` | Artwork detail (opens as modal overlay on top of previous page; direct-load renders full page) |
| `/collections` | User's collections index |
| `/collections/[id]` | Single collection view |
| `/c/[shareId]` | Public shared collection |
| `/settings` | User settings |
| `/studio` | Artist dashboard (default tab: portfolio) |
| `/studio/analytics` | Artist analytics |
| `/studio/settings` | Artist settings |
| `/onboarding` | Artist onboarding flow (stepped) |
| `/sign-in`, `/sign-up` | Clerk-handled auth pages |

Note: pre-built portfolio claim flow (`/claim/[token]`) and admin submission queue (`/admin/submissions`) are deferred to v2 — see `99-deferred.md`. V1 onboards artists via direct outreach to a hand-picked group of 20–30.

## Page-by-page

### Homepage (`/`)

**Purpose:** Immediate entry to discovery. Search-first, with semantic neighborhoods as browsable context.

**Layout (top to bottom):**
1. Hero search block: centered, generous vertical padding. Large search input with placeholder "Search artworks, artists, or drop an image." Camera icon inside the input on the right, opens image upload flow. No other text in the hero — just the search.
2. Semantic neighborhoods section: heading "Explore neighborhoods" with a quiet subheading. Grid of 6 neighborhood cards (2 rows × 3 on desktop, 1 column on mobile). Each card: 3 representative thumbnails arranged asymmetrically, neighborhood name, short description (1 line, ~10 words). Link to `/neighborhoods` at the bottom — "See all neighborhoods →".

   V1 note: neighborhoods are **manually curated** — name, description, and representative artworks set in an admin script or DB seed. Algorithmic clustering (HDBSCAN + LLM labeling) waits until the corpus is large enough to cluster meaningfully (~thousands of artworks). See `99-deferred.md`.
3. Recent additions section: heading "Recently added." Grid of 12 most recently published artworks (4 columns desktop). Same card component as elsewhere.
4. Footer.

**Interactions:**
- Typing in search and pressing enter → navigates to `/search?q=<text>`.
- Clicking camera icon → opens image upload modal. After upload + optional modifier selection → navigates to `/search?image=<id>&modifiers=...`.
- Clicking a neighborhood card → navigates to `/neighborhoods/[slug]`.
- Clicking an artwork card → opens artwork detail modal (URL updates to `/artworks/[id]`).

**Cold-start UX (logged-out / new user):** no personalized recommendations exist yet — the homepage is the same for everyone (curated neighborhoods + recent additions). Personalization surfaces only appear once the user has accumulated enough behavioral signal to compute a taste embedding.

**Data needed:** 6 curated neighborhoods (with 3 representative artworks each), 12 most recent artworks.

### Search results (`/search`)

**Purpose:** Refinement and retrieval. Accepts text, image, or both.

**Layout:**
1. Top nav (search bar reflects current query).
2. Query context bar: shows what was searched ("Showing results for 'moody coastal'" or "Results similar to [uploaded image thumbnail] + 'warmer'"). Includes a "Clear" link and a small "Edit query" affordance.
3. Filters row: collapsed by default, expandable. Pills for: Medium, Price, Size, Orientation, Availability, Color, **Location**. Clicking a pill opens an inline dropdown with options. The Location pill: text input ("Berlin", "London", "New York") with autocomplete from popular artist cities, plus a separate "Near me" toggle that uses browser geolocation (50km radius default, with a slider 5–250km). Applied filters show as removable chips below. Sort dropdown far right: Relevance (default), Newest, Price ↑, Price ↓, **Nearest** (only visible when Near-me is on).
4. Visual search only: modifier buttons row above results — "Moodier", "Warmer", "Cooler", "More minimal", "More textured", "More graphic". Clicking toggles modifier, re-runs search. Selected modifiers highlighted.
5. Results grid: 4 columns desktop, 3 tablet, 2 mobile. Infinite scroll. Each card: image (aspect-ratio preserved), artist name (small), title (if present). Hover reveals save-to-collection icon.
6. Empty state: "No artworks match this search. Try fewer filters, or explore a neighborhood →" with 3 neighborhood cards below.

**Interactions:**
- Filter changes update URL query params and re-fetch.
- Sort changes update URL query params and re-fetch.
- Click artwork → modal (URL to `/artworks/[id]`).
- Save icon → if signed in, opens save-to-collection modal; if not, opens sign-in modal with redirect preserving action.

**Data needed:** ranked artwork list (paginated, cursor-based), facet counts per filter option.

**Map mode** *(shipped 2026-05-29, T-038 G5 + T-041/T-042/T-043)*

A Grid / Map view toggle (small chip strip below the FilterBar) swaps the results grid for a Mapbox GL JS interactive map of venues — galleries + studios where matching artists' work can be seen in person.

- **Pins per `artist_locations` row.** Click → popover with venue name, address, "Showing {artist}" + thumbnail, "View portfolio →" link. Clustered at low zoom (client-side via Mapbox's `cluster: true`).
- **URL is the source of truth.** Pan/zoom updates `?bbox=…` via `history.replaceState` (no history spam). Reload keeps the same view.
- **City pivots above the map** *(T-042)* — horizontal pill strip "London (12) · Berlin (8) · Lisbon (3)" sourced from `/v1/search/map/cities`. Click → jumps to that city's bbox. Solves cold-start when the user lands at world zoom with nothing visible.
- **Near-me button** *(T-043)* — uses browser geolocation API, sets bbox ~5km around the user's coords. Self-hides on browsers without `navigator.geolocation`. PERMISSION_DENIED gets a soft inline message.
- **Artist filter** *(T-041)* — `?artist=<slug>` pins down to a single artist's venues, with a "Showing where to see **{name}** [Clear filter]" pill at the top. Entered via the "See on full map →" CTA on `/artists/[slug]`.
- **Filter semantics** — `q` / `medium` / `location` use the same per-artist match the grid uses: a venue shows if the artist has *any* artwork matching. See `decisions.md` 2026-05-29 — map-search filter semantics.

Falls back to a non-interactive list of pin cards when `NEXT_PUBLIC_MAPBOX_TOKEN` is unset (same fallback shape as the artist-profile map widget).

### Semantic neighborhoods index (`/neighborhoods`)

**Purpose:** Discover the full set of neighborhoods.

**Layout:**
1. Page title "Neighborhoods" with a one-line explanation ("Clusters of visually and conceptually related work.").
2. Grid of all neighborhood cards (same design as homepage). 3 columns desktop, 2 tablet, 1 mobile.

**Data needed:** full neighborhood list with representative thumbnails.

### Single semantic neighborhood (`/neighborhoods/[slug]`)

**Purpose:** Browse one curated cluster.

**Layout:**
1. Header: neighborhood name (large, serif if the design system goes serif for display), description (2–3 sentences, hand-written by curator in v1), representative image strip (6 thumbnails) across the top.
2. Filters row: same as search (Medium, Price, Size, Orientation, Availability, Sort). No text query here.
3. Results grid: same component as search results. Infinite scroll.

**Interactions:** identical to search results.

**Data needed:** neighborhood metadata, ranked artwork list within cluster.

### Artwork detail (modal + `/artworks/[id]`)

**Purpose:** Full view of a single work, with pathways to similar and to the artist.

**Layout (modal):**
- Two-column on desktop: image left (large, zoomable on click), details right. Single-column on mobile: image on top, details below.
- Details column: title, artist name (clickable → artist portfolio), year, medium, dimensions, price + availability (or "Price on inquiry"), description (artist's own words), primary CTA "Inquire" button, secondary "Save to collection" icon.
- Below the main block: "More like this" section with 8 artworks (horizontal scroll on mobile, 4-column grid on desktop).

**Layout (direct-load at `/artworks/[id]`):**
- Same structure as modal but in a normal page layout with the top nav above.
- A "Back" affordance is contextual — if the user arrived via a modal from another page, closing returns to that page. Direct-load shows a normal nav.

**Interactions:**
- Modal opens via client-side navigation (pushes `/artworks/[id]` to URL). Close returns to previous URL.
- Image click → zoomed overlay.
- Save → opens save-to-collection modal (with "create new collection" inline option, see Collections modal spec).
- Inquire → opens inquiry modal (see below).
- Clicking "More like this" artwork → swaps modal to that artwork.
- Clicking artist name → navigates to artist portfolio (closes modal).

**Data needed:** full artwork data, top 8 similar artworks.

### Inquiry modal

**Purpose:** Send a message to the artist.

**Layout:** Short form. Name (pre-filled if signed in), email (pre-filled), message textarea, optional budget range dropdown, submit button. Explanation text: "This goes directly to [Artist Name]. You'll hear back from them directly — we don't take a cut."

**Routing logic:** Backend reads artist's `inquiry_preferences` — direct email (most common), on-platform inbox, or external URL redirect. UI is the same; backend handles delivery.

### Save-to-collection modal

**Purpose:** Let signed-in user save an artwork.

**Layout:**
- List of user's existing collections with checkboxes. Click a collection to add/remove the artwork.
- Below the list: "+ New collection" inline input. Typing a name and pressing enter creates the collection and adds the artwork. Toast confirms.

**If not signed in:** Replaces with a sign-in prompt — "Sign in to save artworks" with Clerk auth trigger. After sign-in, the action completes automatically.

### Artist portfolio (`/artists/[slug]`)

**Purpose:** An artist's gallery-like public page. Links out to their own site.

**Layout:**
1. Header: artist name (large), location (small, muted), short bio (2–3 lines), links row (website, Instagram, other socials — each as a small icon+label), commissioning status badge if accepting.
2. Artist statement section: expandable "Read more" if long. Skipped if empty.
3. Artworks grid: 4 columns desktop, infinite scroll. Same card component as search. Default sort newest-first. Optional tabs: "All", "Available". (No medium tabs for v1 — keeps it simple.)
4. "More like this artist" section at the bottom: heading "Similar artists," 6 artist cards (small avatar, name, 3 thumbnail strip, location). Clicking → artist page.

**Interactions:**
- Clicking an artwork opens the same artwork modal as everywhere else.
- Clicking an external link → opens in new tab, logs `artist_link_clicked_out` event.

**Data needed:** artist profile, paginated artworks, 6 similar artists.

### Collections index (`/collections`)

**Purpose:** User's saved collections.

**Layout:**
1. Page title "Your collections."
2. "+ New collection" button (top right).
3. Grid of collection cards: cover image (first artwork's image, or asymmetric grid of up to 4 thumbnails), collection name, artwork count, privacy indicator (private/public icon). 3 columns desktop.
4. Empty state: "You haven't saved any artworks yet. Explore → [link to homepage]."

**Data needed:** user's collections with cover art and counts.

### Single collection (`/collections/[id]`)

**Purpose:** View one collection.

**Layout:**
1. Header: collection name (editable inline via click-to-edit), description (editable), artwork count, privacy toggle (private/public), share link if public, delete button (icon, confirms).
2. Artworks grid (4 columns desktop). Drag-to-reorder on desktop (deferred to v1.1 — for v1, just show in saved-order).
3. Clicking an artwork opens the detail modal.

**Deferred to v1.1:** notes per artwork within a collection, reorder UI. Schema supports it; UI doesn't yet.

**Data needed:** collection metadata, artworks in collection.

### Public shared collection (`/c/[shareId]`)

Same as single collection but read-only, no edit affordances, no save/delete. Header shows "Collection by [User]" with link to user's public profile if one exists (v1: no public user profile page, so just show display name as text).

### User settings (`/settings`)

**Purpose:** Minimal account management.

**Layout:** Single page, sections stacked:
1. Profile: display name, avatar upload.
2. Account: email (read-only, managed by Clerk), change password (link to Clerk flow).
3. Data: "Export my data" button (emails a JSON dump — v1.1), "Delete account" button (confirms twice).

V1: skip email preferences entirely (no newsletters to opt in/out of).

### Artist onboarding (`/onboarding`)

**Purpose:** Stepped wizard that turns a signed-in user into a published artist. Single page, URL-driven step state (`?step=…`) so refresh is safe and individual steps are deep-linkable.

**Shipped (T-012 Phase 1, 2026-05-28):**

1. **Identity.** Display name (required) + free-text location (optional). Submits to `POST /v1/onboarding/start`, which mints the `artists` row with `status='pending'`, generates a unique slug, and flips `users.is_artist`.
2. **Profile.** Bio, artist statement, website. Wraps the existing `PATCH /v1/studio/settings` endpoint. Skippable.
3. **Artworks.** Reuses the studio `ArtworkEditModal` for create + edit. Skip-friendly — zero artworks at publish time is fine; artists can add more later from `/studio`.
4. **Where to see (locations).** Reuses `StudioLocationsManager` (T-038 G3) — galleries + studios + their geocoded pins.
5. **Review + publish.** Summary of everything entered. "Publish" hits `POST /v1/onboarding/complete` which flips `status: pending → active` (idempotent) and redirects to `/artists/<slug>`.

**Deferred to Phase 2 (blocked on Inngest runtime):**

- Website / Instagram scrape job to pre-fill bio + image URLs (`POST /v1/onboarding/import`)
- Conversational LLM extraction per artwork ("Tell me about this piece — medium, size, what you were thinking about") via `POST /v1/onboarding/extract-metadata`
- "Help me polish this" button on the statement step (`POST /v1/onboarding/polish-statement`)
- Per-step progress persistence (today: each step's data is persisted on submit, but partial state on a step isn't checkpointed)

**Cross-cutting:** `/studio` and `/studio/settings` redirect signed-in non-artists into the wizard. `TopNav` surfaces a "Studio" link for all signed-in users (single link, two destinations — the page-level redirect handles the branch).

### Artist studio (`/studio`, `/studio/analytics`, `/studio/settings`)

**Purpose:** Single "Studio" surface with tabs.

**Tabs:** Portfolio (default), Analytics, Settings.

**Portfolio tab (`/studio`):**
- "+ Add artwork" button (top right). Opens the conversational intake flow as a modal for a single artwork.
- Grid of all artworks with status badges (published/draft). Each has edit (opens same modal pre-filled) and delete actions.
- Bulk upload supported: drag 10 images in, each gets a row in a queue, user can fill minimal info or use LLM chat per-image.
- Filter: "All / Published / Draft."

**Analytics tab (`/studio/analytics`):**
- Top row: 4 stat cards — Views, Saves, Click-outs, Inquiries (last 30 days, with % change vs previous 30).
- Line chart: views over time (last 90 days, daily).
- Top artworks table: artwork thumbnail, title, views, saves, click-outs, inquiries. Sortable columns.
- Referrer breakdown: simple list (Search, Homepage, Neighborhood, Other Artist, Direct).

**Settings tab (`/studio/settings`):**
- Bio, location, website, socials.
- Artist statement (with LLM polish button).
- Commissioning preferences.
- Inquiry routing.
- Portfolio visibility: Published / Unpublished (not deleted — temporarily hidden).
- Account: link to /settings for user-level stuff.

## Components referenced across pages

All the above reduces to a small, reusable component set, listed in the component library doc.

## Questions explicitly deferred

- Editorial / weekly email — not built.
- Public user profile pages — not built.
- Dark mode — not built.
- Drag-to-reorder in collections — schema supports, UI deferred.
- Notes per saved artwork — schema supports, UI deferred.
- Multi-currency — pick USD or GBP for v1 based on your preference, skip conversion.
- PWA / mobile app — responsive web only.

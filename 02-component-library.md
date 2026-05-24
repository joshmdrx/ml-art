# Art Discovery Platform — Component Library (v1)

## Principles

- Minimal surface area. Every new component must justify its existence.
- Composition over configuration. Small, single-purpose components that compose into pages.
- Tailwind for styling; no CSS-in-JS. One design-token source of truth in `tailwind.config.ts`.
- All components typed with TypeScript. Props explicit, no `any`.
- Accessibility baked in — focus traps on modals, keyboard navigation on all interactive elements, ARIA where needed. WCAG AA.
- No visual library dependencies where a primitive will do. Headless UI or Radix Primitives for complex a11y patterns (modals, dropdowns, tabs) — otherwise hand-rolled.

## Design tokens (Tailwind config)

- **Colors:** neutral palette. Background off-white (`#FAFAF8`), surface white, text near-black (`#1A1A1A`), muted (`#6B6B6B`), border (`#E5E5E3`). One accent for interactive state (e.g. `#2D2D2D` on hover, not a bright color). Destructive red used sparingly.
- **Typography:** Sans-serif for UI, serif for display on artwork titles, artist names in headers, and neighborhood names. Recommend Inter (UI) + a quiet serif like Instrument Serif or Fraunces (display).
- **Spacing:** Tailwind default scale. Generous use of `py-16` to `py-24` on section breaks.
- **Radius:** minimal — 0 to 2px on most elements. No rounded buttons.
- **Shadows:** almost none. Use borders for separation instead.
- **Motion:** short, quiet transitions (150–200ms). No bounce, no spring.

## Base components

### `<Button>`

**Props:** `variant: 'primary' | 'secondary' | 'ghost' | 'destructive'`, `size: 'sm' | 'md' | 'lg'`, `disabled`, `loading`, `onClick`, `type`, `children`, `iconLeft`, `iconRight`, `asChild` (for rendering as a link).

**Variants:**
- `primary`: dark bg, light text. One per screen typically.
- `secondary`: bordered, dark text, light bg.
- `ghost`: no border, text only with hover bg.
- `destructive`: red border/text for delete actions.

### `<Input>` / `<Textarea>` / `<Select>`

Standard form inputs. Shared wrapper component `<FormField>` provides label, helper text, error state.

**Props:** `label`, `helperText`, `error`, `required`, plus the standard input attributes.

### `<SearchBar>`

**Props:** `value`, `onChange`, `onSubmit`, `placeholder`, `size: 'hero' | 'nav'`, `onImageUpload` (callback).

- Renders a large input with rounded-none styling, a submit behavior on enter.
- Camera icon inside right edge triggers `onImageUpload`.
- `hero` is used on homepage (large, centered). `nav` is used in sticky top nav (smaller, inline).

### `<Modal>`

Built on Radix Dialog or Headless UI Dialog for accessibility.

**Props:** `open`, `onClose`, `size: 'sm' | 'md' | 'lg' | 'full'`, `title` (optional), `children`, `dismissable` (default true).

- Backdrop click and ESC close by default.
- Focus trapped, returns focus on close.
- Scroll lock on body while open.
- Mobile: slides up from bottom as a sheet; desktop: centered.
- All modals (save-to-collection, inquiry, image upload, artwork detail, add-artwork) share this component.

### `<Toast>` / `<ToastProvider>`

Global toast system. One provider at root layout.

**API:** `toast.success(msg)`, `toast.error(msg)`, `toast.info(msg)`. Dismissable, auto-dismiss after 4s.

Stacked bottom-right on desktop, bottom-center on mobile.

### `<Card>`

Generic container with border and padding. Used as a base for ArtworkCard, NeighborhoodCard, CollectionCard, ArtistCard.

**Props:** `onClick`, `href` (mutually exclusive), `className`, `children`.

### `<ArtworkCard>`

**Props:** `artwork: Artwork`, `onSave?`, `saved?: boolean`, `showArtist?: boolean`, `size: 'sm' | 'md' | 'lg'`.

- Image (lazy-loaded, aspect ratio preserved).
- Below image: artist name (small, muted), title (serif, one line, truncated).
- On hover: save icon top-right fades in.
- Clicking card opens artwork modal (handled by parent).

### `<ArtistCard>`

Compact artist preview. Avatar, name, location, 3-thumbnail strip of their work.

### `<NeighborhoodCard>`

**Props:** `neighborhood: Neighborhood`.

- Asymmetric 3-thumbnail layout on top.
- Name (serif).
- One-line description (muted).
- Entire card is a link.

### `<CollectionCard>`

**Props:** `collection: Collection`, `editable?: boolean`.

- Cover image or 4-thumbnail mosaic.
- Name, artwork count, privacy icon.
- If editable, shows actions menu (...) on hover.

### `<Grid>`

**Props:** `cols: { sm?: number, md?: number, lg?: number }`, `gap`, `children`.

- CSS grid wrapper. Used for all grid layouts.
- Default gap generous (24–32px) to support white-space aesthetic.

### `<FilterBar>`

**Props:** `filters: FilterConfig[]`, `values: FilterValues`, `onChange`, `facetCounts?`.

- Horizontal row of filter pills.
- Each pill opens a dropdown with options and optionally facet counts.
- Applied filters render as removable chips below.
- Sort dropdown far right.

### `<ModifierButtons>`

**Props:** `modifiers: string[]`, `selected: string[]`, `onChange`.

- Visual search only. Row of toggle buttons ("Moodier", "Warmer", etc.).
- Selected state visually distinct (dark fill).

### `<ImageUpload>`

**Props:** `onUpload`, `accept`, `maxSize`.

- Drag-and-drop zone + click to browse.
- Preview thumbnail on upload.
- Handles resize client-side before upload.

### `<Tabs>`

Built on Radix Tabs. Used in /studio.

**Props:** `tabs: { value, label, content }[]`, `defaultValue`, `value`, `onValueChange`.

### `<Chart>`

For studio analytics. Wrap Recharts with project defaults (line-only, minimal axes, no grid). One component `<LineChart>` for v1 — no other chart types needed.

### `<EmptyState>`

**Props:** `title`, `description`, `action?: { label, onClick }`, `illustration?`.

- Centered, muted, low visual weight.
- Used on no-results pages and empty collections.

### `<Avatar>`

**Props:** `src`, `alt`, `size`.

- Circle, with initial fallback if no src.

### `<Tag>` / `<Chip>`

Small pill for medium, tags, applied filters. Removable variant has an × button.

### `<Skeleton>`

Loading placeholder. Used in grids while data loads.

## Layout components

### `<TopNav>`

Sticky, persistent. Contains logo, search bar (nav size), collections link (authed), profile menu / sign-in button.

### `<Footer>`

Minimal. Links row + copyright.

### `<PageLayout>`

Wraps every page. Renders TopNav, page content, Footer.

### `<StudioLayout>`

Wraps `/studio/*` pages. Renders TopNav, tabs bar, page content.

### `<OnboardingLayout>`

Wraps `/onboarding`. Progress indicator at top, step content, sticky bottom nav with back/next.

## Data hooks

Each data fetch is isolated to a hook so components don't contain fetch logic.

- `useArtwork(id)` — single artwork.
- `useArtworks(query)` — search/list with filters.
- `useArtist(slug)` — artist profile + artworks.
- `useNeighborhoods()` / `useNeighborhood(slug)` — cluster data.
- `useCollections()` / `useCollection(id)` — user collections.
- `useSaveArtwork()` — mutation.
- `useInquiry()` — mutation for submitting an inquiry.
- `useStudioArtworks()` / `useStudioAnalytics()` — artist-owned data.

Use TanStack Query (React Query) for all data fetching. Handles caching, invalidation, optimistic updates, retries. Single source of truth for server state.

## State management

- **Server state:** TanStack Query only. No Redux, no Zustand for server data.
- **URL state:** `useSearchParams` for filters, sort, page, modal open state on artwork detail.
- **Local UI state:** `useState` / `useReducer` within the component.
- **Global UI state:** minimal. One Zustand store for toast queue (if not using a library) and possibly a "pending save action" store to preserve intent across sign-in redirects.
- **Auth state:** Clerk provides `useUser()`, `useAuth()` hooks.

## Error and loading patterns

- Every data-fetching component has defined loading, error, and empty states.
- Loading: skeleton grids, skeleton cards. Never spinners as a first choice.
- Error: inline error message with retry button. Toast for transient errors.
- Empty: `<EmptyState>` component.

## Forms

- React Hook Form + Zod for validation.
- Every form defines a Zod schema. Server endpoints validate with the same schema (shared types).
- Submit buttons show loading state; disable on submit.

## Accessibility patterns

- Every interactive element reachable by keyboard.
- Modal focus traps and return focus on close.
- Skip-to-content link in top nav.
- Images always have `alt` — artwork alt is generated from "Title by Artist, Year" format.
- Color contrast AA minimum throughout.
- Reduced motion respected (`prefers-reduced-motion`).

## Testing

For v1 hand-off, minimum:

- Component-level tests on `Button`, `Modal`, `SearchBar`, `ArtworkCard`, `FilterBar` — interaction and a11y.
- E2E smoke test: load homepage → search → open artwork → save to collection → view collection.

Playwright for E2E, Vitest + React Testing Library for components.

## What this library explicitly doesn't include

- Animation library (Framer Motion etc.). CSS transitions only for v1.
- Icon library beyond Lucide (already a Tailwind-friendly default).
- Rich text editor.
- Calendar / date picker (no dates needed beyond year inputs).
- Carousels / sliders (not used in v1).
- Charts beyond single line chart.
- Dark mode tokens.

If any of these become needed later, they get added deliberately, not pre-emptively.

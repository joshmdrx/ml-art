# Contributing

Conventions we've evolved by accident and now write down. Read once, refer
back when something feels ambiguous. Disagreements are fine — open the
discussion, log a follow-up entry in `decisions.md` if we move.

## Docs hierarchy (which file is which)

| File | What it is |
|---|---|
| `README.md` | Reader's first stop. Quick-start, what works today, links out. |
| `STRATEGY.md` | The "why we're building this" doc. Stage plan, open strategic questions. |
| `01-page-spec.md` / `02-component-library.md` / `03-api-data-spec.md` | **Aspirational** product brief — describes the *intended* v1 product, not necessarily current state. See `decisions.md` 2026-05-27. |
| `04-stack-and-infra.md` | Architectural reference (hosting, runtimes, services). |
| `05-local-dev.md` | Dev-environment runbook. |
| `99-deferred.md` | Things we've explicitly scoped out of v1. |
| `CHANGELOG.md` | **Truth for what was built.** One entry per ship, chronological. |
| `decisions.md` | **Truth for why we built it that way.** One entry per significant choice; format documented in-file. |
| `TODO.md` | Live engineering backlog. Things land here on creation, move to `CHANGELOG` on completion. |
| `TESTING.md` | Test-tier strategy + current counts. |
| `COST.md` | Spend monitoring policy + paid-service cost ceilings. |

The specs (01–03) are *not* maintained line-for-line with code. When code
deviates: log a decision and update the CHANGELOG, not the spec. The
spec only changes when (a) it's materially wrong about shipped behavior
or (b) we're scoping a new major surface.

## Local setup

See `README.md` for the quickstart. TL;DR:

```bash
make setup        # docker up + migrate + seed
make dev          # api + web together
make check        # fmt + clippy + typecheck + lint (no tests)
make test-all     # everything including Playwright
```

Pre-commit hooks (lefthook) run automatically on `git commit`. If you
haven't installed lefthook yet:

```bash
brew install lefthook          # or your platform's equivalent
lefthook install               # one-time, writes the git hooks
```

The hooks run the same checks CI runs, scoped to the files you touched.
Bypass at your peril with `git commit --no-verify` — but if you do, CI
will catch you. **Do not run `--no-verify` in agent sessions** unless
you've manually confirmed the underlying issue is unrelated and the
human will fix it.

## Code conventions

### Rust

- **Module-level docs (`//! …`) at the top of every public module.** A
  paragraph on what it does, who calls it, what the failure modes are.
  See `core::middleware::rate_limit` for a good example.
- **Errors are `ApiError`**, defined in `core::error`, rendered as
  RFC 7807 problem+json. Don't `panic!` from a handler. Don't return
  raw `StatusCode` — wrap in the appropriate variant.
- **`unwrap()` / `expect()` are allowed only when failure is structurally
  impossible** (e.g. `NonZeroU32::new(n.max(1)).expect("max(1) is non-zero")`).
  The string passed to `expect` should explain why it can't fail. In
  handler paths, never; in `build_app` / `Config::load`, occasionally.
- **Test-mode constructors live next to production constructors**
  (`JwtVerifier::for_tests`, `Embedder::with_fixed_vector`,
  `Config::for_tests`). Always explicit — never gated on an env var
  that could leak into production.
- **Row types live in handler files, not in `core::models`.** `core::models`
  is for types serialized over the wire. SQL row structs (`CollectionRow`,
  `ArtworkRow`) are private to the handler module that owns them.
- **Auth is an extractor.** New handlers requiring a signed-in user take
  `User(user): User` in the signature. Don't call
  `auth::authenticate(...)` inline anymore — that pattern was retired in
  `decisions.md` 2026-05-27.
- **Dynamic SQL uses `AssertSqlSafe(format!(...))` + `PgArguments::add`.**
  See `search.rs` or `neighborhoods.rs::detail` for the pattern. Build
  the clauses + args together so parameter indices stay aligned with the
  bind order.

### TypeScript / React

- **Server-side data fetching uses `lib/api.ts` helpers.** They handle
  Bearer + anon_id forwarding and consistent error shape. Don't `fetch`
  the API directly from page components.
- **Write paths go through server actions** (`web/src/app/actions/*.ts`),
  not client-side fetch. Keeps Clerk tokens server-side.
- **Errors call `reportError(err, context)`** from `lib/reportError.ts`,
  not `console.error` directly. The shim lets us swap in Sentry without
  touching call sites. New code using bare `console.error` will fail
  code review.
- **Client components opt in with `"use client"`** at the top of the
  file. Server-first by default — only the parts that need browser APIs
  (state, effects, event handlers) go client.
- **TypeScript is strict.** No `any` (`unknown` is fine — it forces a
  narrow). Discriminated unions for state machines (see `SaveModal`'s
  `State` type).

### SQL / migrations

- **Filename: `NNNN_topic.sql`** in `db/migrations/`. Sequential numbers,
  short topic name. Once shipped, never modify — write a follow-up
  migration.
- **One concern per migration.** Adding a column to artworks and a new
  table for events should be two files, not one.
- **All tables have `created_at` + `updated_at` timestamptz.** Soft-delete
  pattern uses `deleted_at timestamptz`.
- **`UUID` keys use v7** for natural sort order when read in batches.

## TODO comments

Every inline `TODO` references a `TODO.md` ticket:

```rust
// TODO(T-032): replace with Resend HTTP call once the secret is wired
```

Bare `TODO:` and `FIXME` are not allowed and will be rejected by
pre-commit + CI. If you have a thought that doesn't merit a ticket,
either:

- Add it as a ticket in `TODO.md` (low overhead — one bullet)
- Log it as a follow-up entry in `decisions.md` if it's a known
  tradeoff we accepted

See `decisions.md` 2026-05-27 — TODO comment format for the rationale.

## Tests

Three tiers, all documented in `TESTING.md`:

1. **Rust integration** (`api/crates/api-search/tests/*_test.rs`) — real
   Postgres via `sqlx::test`, real Axum stack, stubbed external services.
2. **Vitest unit** (`web/src/__tests__/*.test.ts`) — pure helpers and
   format functions. No DOM, no network.
3. **Playwright E2E** (`e2e/tests/*.spec.ts`) — full stack against a
   running local dev environment.

Naming: tests describe behavior, not method. `collections_create_then_list`
beats `test_create`. New tests go in the existing `*_test.rs` for that
module unless the file is over ~300 lines, then split by feature.

A new behavior PR should add tests at the lowest tier that can prove
the change. Don't add a Playwright test if a Vitest unit + Rust
integration combo would catch the regression.

## Commits & PRs

- **Commits**: present-tense, imperative subject line (`add filter param
  to neighborhood detail`, not `added`). Body explains why, not what.
- **No bot-attribution in commit messages** unless asked. The codebase
  uses no "Co-Authored-By" lines.
- **PRs**: link the `T-NNN` ticket(s) in the description. CI must be
  green before merge. No `--no-verify` to bypass pre-commit; if a check
  is wrong, fix the check.

## Decisions log

Anything that's:

- An architectural choice with reasonable alternatives,
- A "we'll do X for now and Y later" deferral,
- A pattern we want every future contributor to follow,

…goes in `decisions.md` with the standard `Context / Decided /
Alternatives / Why / Reversibility` shape. Entries are append-only;
revisions add a new dated entry rather than editing in place. Keep them
short — paragraph each is fine; ADRs are not the goal.

## When something feels wrong

Open it as a discussion. Either:

1. We learn something and update this file + log a decision, or
2. We've documented the rationale and the asker now knows.

Both outcomes are wins. The thing not to do is silently work around the
convention — that's how documentation drift starts.

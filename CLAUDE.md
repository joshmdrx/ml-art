# CLAUDE.md — read this first

You are an LLM agent contributing to **Wander** (wander.gallery), a
discovery platform for independent contemporary artists. Rust API
(`api/`) + Next.js web (`web/`) + Python ML pipelines (`ml/`) +
Terraform infra (`infra/`).

**Before writing code, read the docs for the surface you're touching.**
The codebase has accumulated real conventions; ignoring them produces
PRs that look out-of-band and waste a review cycle.

---

## What to read, by surface

| Surface | Authoritative doc | What it covers |
|---|---|---|
| **Web UI** (forms, modals, dialogs, toasts, errors) | [`docs/ui-patterns.md`](./docs/ui-patterns.md) | Validation pattern (JS-only, never HTML), `useConfirm()` for yes/no, `toast.success/error` from sonner, modal close-on-success rule, multi-step flows via `setOpen(newId)` from the parent. **ESLint enforces the most fragile rules.** |
| **TypeScript / React** | [`CONTRIBUTING.md` → Code conventions → TypeScript / React](./CONTRIBUTING.md) | Client components must call **server actions** (not `lib/api.ts` directly). Server actions live in `web/src/app/actions/*.ts`. Errors call `reportError(err, context)`, never `console.error`. `"use client"` is opt-in. |
| **Rust / API** | [`CONTRIBUTING.md` → Code conventions → Rust](./CONTRIBUTING.md) | Module-level `//! …` docs required. Errors are `ApiError`. Auth is the `AuthedUser` extractor. Dynamic SQL via `AssertSqlSafe(format!(...))` + `PgArguments`. Row structs stay private to handler modules; `core::models` is wire types only. |
| **DB / migrations** | [`CONTRIBUTING.md` → Code conventions → SQL / migrations](./CONTRIBUTING.md) | `NNNN_topic.sql` filenames, one concern per migration, never edit shipped ones. All tables get `created_at` + `updated_at`; soft delete via `deleted_at`. UUID v7 keys for batch sort order. |
| **Background jobs** | [`CONTRIBUTING.md` → Background jobs](./CONTRIBUTING.md) | Anything outside a request goes through `core::jobs::JobEvent` + a handler arm in `core::jobs::handle`. Existing examples: `ArtistLocationGeocode`, `InquirySendVerification`, `EventLog`. |
| **Tests** | [`TESTING.md`](./TESTING.md) | Three tiers: Rust integration via `sqlx::test`, Vitest unit, Playwright E2E. Add at the lowest tier that can prove the change. |
| **Why we built it that way** | [`decisions.md`](./decisions.md) | Append-only record of significant choices. **Read the recent entries** before making structural calls — most things you'd be tempted to redesign are already discussed. |
| **What ships when** | [`TODO.md`](./TODO.md) (live backlog) + [`CHANGELOG.md`](./CHANGELOG.md) (shipped) | New tickets land in TODO; shipped work moves to CHANGELOG. The `T-NNN` prefix is the canonical ticket id used in commits and comments. |
| **Strategy / scope** | [`STRATEGY.md`](./STRATEGY.md), [`99-deferred.md`](./99-deferred.md) | Stage plan + things we've explicitly de-scoped. Worth checking before proposing major work. |

The full docs hierarchy (per-spec, per-area) is enumerated at the top
of [`CONTRIBUTING.md`](./CONTRIBUTING.md) under "Docs hierarchy".

---

## Working rhythm

- **`make check`** = fmt + clippy + typecheck + lint. Run before
  committing.
- **`make test-all`** = everything including Playwright. CI runs
  the same.
- **Pre-commit hooks** (lefthook) run scoped checks automatically.
  Do not bypass with `--no-verify` in agent sessions.
- **One concern per commit.** Tests + docs + code for the same
  ticket can co-exist in one commit; bug fixes and unrelated tidy-
  ups don't.
- **Commit messages reference the ticket** (`feat(T-058.2): …`,
  `fix(T-057): …`). The format is loose otherwise.

---

## Easy-to-miss conventions (the ones I've personally violated)

If you're new to the codebase, scan these — they're the doc-violation
modes that produced PRs we had to redo:

- **Client components calling `lib/api.ts` directly.** Next.js refuses
  to bundle Clerk's server-only modules into client chunks. Wrap the
  `lib/api` call in a server action in `web/src/app/actions/*.ts`.
  See `actions/studio.ts` for the canonical pattern.
- **Custom inline confirmations / modals for yes/no.** Use
  `useConfirm()` from `@/components/ui/ConfirmDialog`. ESLint bans
  `window.confirm`/`alert`/`prompt`.
- **`console.error` in app code.** Use `reportError(err, context)`
  from `@/lib/reportError`. The shim is where Sentry hooks in.
- **HTML `min`/`max`/`required` on inputs.** Validate in JS, render
  `<FieldError message={err} />` from `@/components/ui/FieldError`.
- **Keeping a save modal open after a successful save.** Close on
  success + `toast.success("…")` from sonner. Multi-step flows
  (create → add detail) re-open via `setOpen(newId)` from the parent
  with an explicit one-line comment.
- **Bare `TODO:` / `FIXME` in code.** Every inline TODO references a
  ticket: `// TODO(T-077): …`. Pre-commit rejects bare ones.
- **Production migrations.** `scripts/migrate.sh` is **local-only**
  (works against the docker postgres). Prod migrations are applied
  manually via `psql` against the Neon DB URL. Deploys don't run
  them automatically — apply *before* the api deploy if the new code
  reads/writes new columns.

---

## When in doubt

- If a convention isn't documented, ask. Don't invent.
- If a convention exists but feels wrong, log the rethink in
  `decisions.md` rather than silently breaking it.
- If a recent commit looks similar to what you're about to do, copy
  its shape rather than re-deriving from first principles.

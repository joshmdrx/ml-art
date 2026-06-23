# UI patterns

Conventions for forms, dialogs, and user feedback across the web app.
The goal is that every screen feels like it came from the same product.
ESLint enforces the most fragile rules; the rest is on code review.

If you're writing a screen and reach for any of the things in the
"Don't do" lists below, pause and pick the pattern from the
corresponding "Do" column.

---

## Form validation

**Single principle:** validation is JavaScript, not HTML.

The browser's native validation surfaces (HTML `min`/`max`/`required`/
`pattern`) trigger styled-by-the-browser tooltips that don't match
anything else in our app. Mixing them with our own inline red error
text on the same screen looks (and is) broken.

| Don't | Do |
|---|---|
| `<input type="number" min={1} max={5000} required />` | `<input type="number" />` + check the value in your submit handler |
| Render bespoke `<p style={{color: 'red'}}>{err}</p>` | `<FieldError message={err} />` |
| Style validation messages per-screen | Trust `<FieldError>` to own the styling — never reach inside |

`<FieldError message={null}>` returns null, so call sites stay compact:

```tsx
<FieldError message={dimsError} />
```

There's one practical exception, kept narrow: a *pure HTML form* that
posts directly to the server with no JS submit handler can keep
`required` on plain text fields, because there's no JS to lift the
validation into. `InquiryModal` is the only current example. If you
add a second, you're probably better off adding JS validation.

---

## Confirm dialogs

**Single principle:** never use `window.confirm` / `alert` / `prompt`.
ESLint will fail your build.

The native dialogs are unstyled, don't focus-trap, and on mobile the
"Cancel" / "OK" labels are out of our control. We use a
promise-based hook backed by Radix's AlertDialog primitive instead.

```tsx
import { useConfirm } from "@/components/ui/ConfirmDialog";

const confirm = useConfirm();

async function onPublish() {
  const ok = await confirm({
    title: "Publish without dimensions?",
    description: "Buyers won't be able to filter your work by size.",
    confirmLabel: "Publish anyway",
  });
  if (!ok) return;
  // …proceed
}
```

For destructive operations (delete / archive / discard), pass
`destructive: true` to render the confirm button in red.

The provider (`<ConfirmDialogProvider>`) is mounted at the app root
already. Anywhere below it, `useConfirm()` just works.

---

## Feedback for async actions

**Single principle:** confirm outcomes outside the form, surface
errors inside it.

| Outcome | Where it goes |
|---|---|
| Async action succeeded (save, delete, follow, send) | `toast.success("…")` from sonner |
| Async action failed at the field level (validation) | `<FieldError>` next to the field |
| Async action failed at the form level (network / 500) | Inline alert banner inside the form |
| Long-running action with visible latency | `toast.promise(p, { loading, success, error })` |

```tsx
import { toast } from "sonner";

await saveArtwork(detail);
toast.success("Saved");
```

The `<Toaster />` is at the app root with `richColors closeButton`
defaults. Don't reach inside its styling per-call — adjust the
toaster prop if a consistent change is wanted everywhere.

Why never toast for validation errors: toasts disappear. Validation
messages must persist until the user has read and acted on them, and
they must be visually anchored to the input that caused them.

---

## Modal / dialog behaviour

| Don't | Do |
|---|---|
| Keep a save modal open after a successful save | Close on success; surface the outcome via toast |
| Reopen the modal in a different mode silently | If a multi-step flow needs follow-up (e.g. create-then-add-image), `setOpen(newId)` explicitly, with a one-line comment |
| Use Radix `Dialog` for a yes/no question | Use `useConfirm()` (built on Radix `AlertDialog`) |
| Use Radix `AlertDialog` for an edit form | Use Radix `Dialog` |

`Dialog` and `AlertDialog` are different primitives in Radix. The
quick mnemonic: AlertDialog *blocks* the page until the user picks
one of N options; Dialog is a generic modal that can be dismissed by
clicking outside or hitting Escape. Confirms are AlertDialogs.

---

## Enforcement

- **ESLint** bans `window.confirm` / `alert` / `prompt` and their bare
  identifier forms — see `web/eslint.config.mjs`. Build fails if you
  reintroduce them.
- **Code review** is the rest. If you see HTML `min`/`max`/`required`
  attributes on inputs with JS handlers, ask the author to lift the
  rule into the submit handler + `<FieldError>`.
- **Decisions** for the underlying choices (sonner over react-hot-toast,
  AlertDialog vs custom modal, validation pattern) are recorded in
  `decisions.md` 2026-06-22 — Feedback primitives + form validation.

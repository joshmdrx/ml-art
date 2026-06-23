/**
 * Inline form-field error renderer (T-071).
 *
 * Every inline validation message in the app goes through this so the
 * styling can't drift between modals / forms / pages. Returns `null`
 * when there's no message, so call sites stay compact:
 *
 *   <FieldError message={dimsError} />
 *
 * The `role="alert"` is what screen readers latch onto — important for
 * the on-submit validation flow where the message appears after the
 * user has already left the field.
 *
 * See `docs/ui-patterns.md` for the full pattern (when inline vs toast
 * vs top-banner; never use HTML `min`/`max`/`required`/`pattern` for
 * validation that has a JS handler).
 */
export function FieldError({ message }: { message: string | null | undefined }) {
  if (!message) return null;
  return (
    <p role="alert" className="mt-1 text-xs text-red-600">
      {message}
    </p>
  );
}

/**
 * Error-reporting shim. Today wraps `console.error` with a structured
 * prefix and context object. When Sentry (or Axiom, or whatever) gets
 * wired pre-launch, only this file changes — call sites stay put.
 *
 * Convention: call this from `catch` blocks where you'd previously have
 * written `console.error("...failed", e)`. The `context` arg captures
 * the surface and any IDs that help triage; pick fields a future
 * dashboard would group by.
 *
 * See `decisions.md` 2026-05-27 — Error reporter shim.
 */

/**
 * Normalize-and-report an error.
 *
 * Calls `console.error` with a `[err]` prefix and a JSON context blob so
 * Vercel function logs are at least grep-friendly. Returns void; never
 * throws (a reporter that fails its job is worse than a silent one).
 */
export function reportError(
  err: unknown,
  context?: Record<string, unknown>
): void {
  try {
    const message = err instanceof Error ? err.message : String(err);
    const stack = err instanceof Error ? err.stack : undefined;
    console.error("[err]", message, {
      ...(stack ? { stack } : {}),
      ...(context ?? {}),
    });
  } catch {
    // Reporter never throws. If String(err) panics on a hostile
    // toString(), we just drop the report.
  }
}

/**
 * Convert a caught error into copy safe to render to a user, while
 * shipping the raw error to the reporter for diagnosis.
 *
 * Always returns `fallback` — we deliberately never render `e.message`
 * directly. Server-action failures, API HTTP errors, framework
 * exceptions, and "internal error in Server Components render"
 * messages all surface as actionable, neutral copy ("Couldn't load
 * collections.", "Couldn't save settings.", etc). The actual details
 * still land in CloudWatch / Sentry via `reportError`.
 *
 * The intent matters: a privacy-conscious surface should never leak
 * internal class names, framework jargon, or stack traces to the
 * person trying to use the product.
 */
export function toUserMessage(
  err: unknown,
  fallback: string,
  context?: Record<string, unknown>
): string {
  reportError(err, context);
  return fallback;
}

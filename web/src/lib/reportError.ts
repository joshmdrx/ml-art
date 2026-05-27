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

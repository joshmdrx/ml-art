/**
 * Loose URL normalization for user-typed website fields.
 *
 * Artists won't type `https://`. They'll type `guitardojo.app` or
 * `www.guitardojo.app`. The server-side validator wants a real
 * scheme; we mediate by prepending `https://` when missing.
 *
 * Returns:
 *   - `null` for empty/whitespace-only input (caller treats as "clear")
 *   - the input verbatim when it already starts with `http://` or
 *     `https://`
 *   - `https://<input>` otherwise
 *
 * We intentionally don't validate beyond "looks like it has a host."
 * The server's `validate_website` is the contract; we just make sure
 * we never send a bare hostname that the server would reject.
 *
 * Things we deliberately DON'T do (yet):
 *   - Hostname structural validation (must contain a dot) — bites on
 *     intranet hosts and looks pedantic
 *   - Force https over http — some artists may legitimately link to
 *     legacy http-only sites; let the server reject if needed
 *   - URL parsing via `new URL()` — would throw on partial input
 *     while the user is mid-type
 */
export function normalizeWebsiteUrl(input: string): string | null {
  const trimmed = input.trim();
  if (trimmed.length === 0) return null;
  if (/^https?:\/\//i.test(trimmed)) return trimmed;
  return `https://${trimmed}`;
}

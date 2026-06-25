/**
 * Client-side analytics emitter (T-050.3).
 *
 * Web call sites import `track(name, properties)` — that pushes the
 * event into an in-memory buffer and schedules a flush. The buffer
 * flushes when ANY of:
 *
 *   - it hits {@link FLUSH_AT_COUNT} entries
 *   - the {@link FLUSH_AFTER_MS} timer fires
 *   - the tab visibility flips to "hidden" (user switched tabs / closed)
 *   - the browser fires `pagehide` (real navigation away)
 *
 * Flush POSTs the batched payload to `/v1/events`. The server derives
 * `anonymous_id` from the cookie + `user_id` from Clerk — the client
 * never sets them. The server-side allowlist gates which names are
 * acceptable from the client (see `events::CLIENT_ALLOWED` in the api).
 *
 * Failure semantics:
 *   - Network failure is silently dropped. Analytics must never break
 *     a real user flow; we don't retry, we don't surface errors.
 *   - The endpoint validates names — typos / removed names 400, but
 *     we don't surface that either. `reportError` only fires for
 *     unexpected throws.
 *
 * SSR safety:
 *   - `track` returns immediately when window is undefined. Server
 *     components calling `track` is a noop, not a crash.
 *
 * @see `core::events::EventName` for the canonical list of names.
 * @see `api-search::events::ingest` for the server-side handler.
 */

import { reportError } from "@/lib/reportError";

/** Mirrors the server-side `EventName` snake_case discriminator. Only
 *  the two client-allowed names are typed here; adding a name requires
 *  flipping both the server allowlist AND adding it to this union. */
export type ClientEventName = "modifier_applied" | "inquiry_started";

interface QueuedEvent {
  name: ClientEventName;
  properties: Record<string, unknown>;
}

const FLUSH_AT_COUNT = 10;
const FLUSH_AFTER_MS = 5_000;
const MAX_QUEUE = 50; // matches server-side MAX_BATCH

let queue: QueuedEvent[] = [];
let flushTimer: ReturnType<typeof setTimeout> | null = null;
let listenersAttached = false;

/** Enqueue an event. Triggers an immediate flush if the queue hits
 *  the count threshold; otherwise schedules a 5s flush. SSR-safe
 *  (returns when window is undefined). */
export function track(
  name: ClientEventName,
  properties: Record<string, unknown> = {},
): void {
  if (typeof window === "undefined") return;

  // Hard ceiling — past MAX_QUEUE we drop NEW events rather than
  // grow the buffer unboundedly. Matches the server's MAX_BATCH so
  // a flush won't 400 for being too big.
  if (queue.length >= MAX_QUEUE) return;

  queue.push({ name, properties });

  attachLifecycleListeners();

  if (queue.length >= FLUSH_AT_COUNT) {
    flush();
  } else if (flushTimer === null) {
    flushTimer = setTimeout(flush, FLUSH_AFTER_MS);
  }
}

/** Force-flush the buffer. Safe to call multiple times; no-op when
 *  empty. Exposed for the lifecycle listeners + tests; consumers
 *  should usually rely on `track`'s built-in scheduling. */
export function flush(): void {
  if (typeof window === "undefined") return;
  if (flushTimer !== null) {
    clearTimeout(flushTimer);
    flushTimer = null;
  }
  if (queue.length === 0) return;

  const batch = queue;
  queue = [];

  // fetch().catch(noop) — analytics is fire-and-forget. We use
  // `keepalive` so an in-flight flush survives a tab close.
  fetch("/api/events", {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify({ events: batch }),
    keepalive: true,
  }).catch((e) => {
    // Don't re-queue — retries would compound the failure if
    // /v1/events is degraded, AND would surface duplicate events
    // when intermittent.
    reportError(e, { surface: "events-flush", count: batch.length });
  });
}

function attachLifecycleListeners(): void {
  if (listenersAttached) return;
  listenersAttached = true;

  // pagehide fires before unload AND when the page is bfcached;
  // beforeunload misses bfcache. Use pagehide for the close path.
  window.addEventListener("pagehide", flush);

  // visibilitychange catches tab-switching — the user is "done with
  // this page" even if they didn't navigate away.
  document.addEventListener("visibilitychange", () => {
    if (document.visibilityState === "hidden") flush();
  });
}

/** Test-only — resets the in-memory queue + timer. Vitest sets this
 *  between cases so leaks from one test don't influence the next. */
export function __resetForTests(): void {
  queue = [];
  if (flushTimer !== null) {
    clearTimeout(flushTimer);
    flushTimer = null;
  }
  listenersAttached = false;
}

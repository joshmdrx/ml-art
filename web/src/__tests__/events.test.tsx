// @vitest-environment happy-dom

/**
 * Client-events batcher (T-050.3) — verifies the queue + flush
 * semantics. Doesn't try to test the network — `fetch` is stubbed
 * to capture the batched body, which is what consumers actually
 * care about.
 */

import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { __resetForTests, flush, track } from "@/lib/events";

interface CapturedFetch {
  url: string;
  body: { events: Array<{ name: string; properties: Record<string, unknown> }> };
}

function stubFetch(captured: CapturedFetch[]): void {
  vi.stubGlobal(
    "fetch",
    vi.fn(async (url: string, init?: RequestInit) => {
      captured.push({
        url,
        body: JSON.parse(init?.body as string),
      });
      return new Response(null, { status: 202 });
    }),
  );
}

describe("events client", () => {
  let captured: CapturedFetch[];

  beforeEach(() => {
    captured = [];
    stubFetch(captured);
    vi.useFakeTimers({ shouldAdvanceTime: true });
  });

  afterEach(() => {
    vi.useRealTimers();
    vi.unstubAllGlobals();
    __resetForTests();
  });

  it("flushes immediately when the queue hits the count threshold", async () => {
    for (let i = 0; i < 10; i++) {
      track("modifier_applied", { codes: [`code-${i}`] });
    }
    // The 10th `track` triggered a flush synchronously (count threshold).
    // Allow the promise microtask to resolve.
    await vi.runAllTimersAsync();
    expect(captured).toHaveLength(1);
    expect(captured[0].body.events).toHaveLength(10);
    expect(captured[0].url).toBe("/api/events");
  });

  it("flushes on timer when below the count threshold", async () => {
    track("modifier_applied", { codes: ["one"] });
    track("inquiry_started", { artwork_id: "abc" });
    expect(captured).toHaveLength(0);

    // Advance past the 5s flush timer.
    await vi.advanceTimersByTimeAsync(5_000);
    expect(captured).toHaveLength(1);
    expect(captured[0].body.events.map((e) => e.name)).toEqual([
      "modifier_applied",
      "inquiry_started",
    ]);
  });

  it("flush() is a no-op when the queue is empty", async () => {
    flush();
    await vi.runAllTimersAsync();
    expect(captured).toHaveLength(0);
  });

  // No test for the 50-event hard ceiling. The count-threshold flush
  // drains the queue every 10 events in normal use, so MAX_QUEUE is
  // a defensive backstop against a tight-loop bug, not an observable
  // behaviour we can practically reproduce.

  it("flushes on visibilitychange → hidden", async () => {
    track("modifier_applied", { codes: ["a"] });
    expect(captured).toHaveLength(0);

    // Simulate the user switching tabs.
    Object.defineProperty(document, "visibilityState", {
      value: "hidden",
      configurable: true,
    });
    document.dispatchEvent(new Event("visibilitychange"));
    await vi.runAllTimersAsync();
    expect(captured).toHaveLength(1);
  });
});

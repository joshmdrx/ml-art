"use client";

/**
 * Visual-search modifier row. Renders one toggle button per registered
 * modifier; clicking adds or removes the modifier from the URL's
 * `modifiers` parameter. Same URL-driven pattern as `FilterBar`.
 *
 * Only shown when the URL carries `image_upload_id` — modifiers without
 * a visual anchor are a server-side 400.
 */

import { useRouter, useSearchParams } from "next/navigation";
import { useTransition } from "react";
import { clsx } from "clsx";
import type { SearchModifier } from "@/lib/api";
import { track } from "@/lib/events";

export function ModifierBar({ modifiers }: { modifiers: SearchModifier[] }) {
  const router = useRouter();
  const searchParams = useSearchParams();
  const [isPending, startTransition] = useTransition();

  // Parse the current selection from the URL. Trailing commas + unknown
  // names tolerated server-side; we just normalize the display set.
  const active = parseActive(searchParams.get("modifiers"));

  function toggle(name: string) {
    const next = new Set(active);
    if (next.has(name)) next.delete(name);
    else next.add(name);
    const usp = new URLSearchParams(searchParams);
    if (next.size === 0) usp.delete("modifiers");
    else usp.set("modifiers", Array.from(next).join(","));
    const qs = usp.toString();
    // T-050.3 — emit on toggle. `codes` is the set AFTER the toggle so
    // analytics see the effective state, not the diff. Cleared selections
    // still fire (empty codes array) — "user cleared modifiers" is a
    // distinct signal from "never engaged."
    track("modifier_applied", {
      codes: Array.from(next),
      toggled: name,
    });
    startTransition(() => router.push(`/search${qs ? `?${qs}` : ""}`));
  }

  if (modifiers.length === 0) return null;

  return (
    <div
      role="toolbar"
      aria-label="Visual-search modifiers"
      className={clsx(
        "mb-6 flex flex-wrap items-center gap-2",
        isPending && "opacity-60"
      )}
    >
      <span className="text-xs text-muted mr-1">Modify:</span>
      {modifiers.map((m) => {
        const isActive = active.has(m.name);
        return (
          <button
            key={m.name}
            type="button"
            aria-pressed={isActive}
            onClick={() => toggle(m.name)}
            disabled={isPending}
            className={clsx(
              "px-3 py-1.5 text-sm border transition-colors",
              isActive
                ? "border-foreground bg-foreground text-background"
                : "border-border bg-surface hover:bg-background"
            )}
          >
            {m.label}
          </button>
        );
      })}
    </div>
  );
}

function parseActive(raw: string | null): Set<string> {
  if (!raw) return new Set();
  return new Set(
    raw
      .split(",")
      .map((s) => s.trim())
      .filter(Boolean)
  );
}

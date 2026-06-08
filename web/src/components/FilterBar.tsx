"use client";

/**
 * Filter pill row used by `/search` and `/neighborhoods/[slug]`. Reads
 * filter state from the URL via `useSearchParams`, writes via
 * `router.push(basePath + "?" + nextParams)` — keeps everything
 * link-shareable and back/forward friendly.
 *
 * Per-surface configuration is passed in via `availableFilters` because
 * the neighborhood page doesn't get a `location` pill (the slug already
 * pins location). All four pills work identically; the layout just hides
 * the ones that aren't enabled.
 */

import { useRouter, useSearchParams } from "next/navigation";
import { useTransition, useState, useRef, useEffect } from "react";
import * as DropdownMenu from "@radix-ui/react-dropdown-menu";
import { clsx } from "clsx";
import {
  applyFilterParam,
  AVAILABILITY_OPTIONS,
  bucketTokenFromPriceParams,
  MEDIUM_OPTIONS,
  PRICE_BUCKETS,
  priceParamsFromToken,
  type FilterKind,
} from "@/lib/filterBar";

interface FilterBarProps {
  /** Subset of pills to render. Order is preserved in the layout. */
  availableFilters: FilterKind[];
  /**
   * Path the FilterBar navigates to when a pill changes — typically the
   * page's own path (`/search` or `/neighborhoods/<slug>`). Lets the
   * neighborhood page keep its slug in the URL across filter changes.
   */
  basePath: string;
}

export function FilterBar({ availableFilters, basePath }: FilterBarProps) {
  const router = useRouter();
  const searchParams = useSearchParams();
  const [, startTransition] = useTransition();

  /** Push a single-key URL update through the router, preserving everything else. */
  function push(update: Record<string, string | null | undefined>) {
    const next = applyFilterParam(new URLSearchParams(searchParams), update);
    const url = next ? `${basePath}?${next}` : basePath;
    startTransition(() => router.push(url));
  }

  // Current values from the URL — these drive the "active" rendering.
  const currentMedium = searchParams.get("medium") || undefined;
  const currentAvailability = searchParams.get("availability") || undefined;
  const currentLocation = searchParams.get("location") || undefined;
  const priceMin = numOrUndef(searchParams.get("price_min"));
  const priceMax = numOrUndef(searchParams.get("price_max"));
  const currentPriceToken = bucketTokenFromPriceParams(priceMin, priceMax);

  return (
    <div
      className="mb-6 flex flex-wrap items-center gap-2"
      role="toolbar"
      aria-label="Filters"
    >
      {availableFilters.includes("medium") && (
        <PillMenu
          label={currentMedium ? `Medium: ${currentMedium}` : "Medium"}
          active={Boolean(currentMedium)}
          onClear={() => push({ medium: null })}
        >
          {MEDIUM_OPTIONS.map((m) => (
            <DropdownMenu.Item
              key={m}
              onSelect={() => push({ medium: m })}
              className="px-3 py-1.5 text-sm cursor-pointer hover:bg-background focus:bg-background focus:outline-none"
            >
              {m}
            </DropdownMenu.Item>
          ))}
        </PillMenu>
      )}

      {availableFilters.includes("price") && (
        <PillMenu
          label={
            currentPriceToken
              ? `Price: ${PRICE_BUCKETS.find((b) => b.token === currentPriceToken)?.label}`
              : "Price"
          }
          active={Boolean(currentPriceToken)}
          onClear={() =>
            push({ price: null, price_min: null, price_max: null })
          }
        >
          {PRICE_BUCKETS.map((b) => (
            <DropdownMenu.Item
              key={b.token}
              onSelect={() => {
                const p = priceParamsFromToken(b.token);
                push({
                  price: b.token,
                  price_min: p?.price_min?.toString() ?? null,
                  price_max: p?.price_max?.toString() ?? null,
                });
              }}
              className="px-3 py-1.5 text-sm cursor-pointer hover:bg-background focus:bg-background focus:outline-none"
            >
              {b.label}
            </DropdownMenu.Item>
          ))}
        </PillMenu>
      )}

      {availableFilters.includes("availability") && (
        <PillMenu
          label={
            currentAvailability
              ? `${AVAILABILITY_OPTIONS.find((o) => o.value === currentAvailability)?.label ?? currentAvailability}`
              : "Availability"
          }
          active={Boolean(currentAvailability)}
          onClear={() => push({ availability: null })}
        >
          {AVAILABILITY_OPTIONS.map((o) => (
            <DropdownMenu.Item
              key={o.value}
              onSelect={() => push({ availability: o.value })}
              className="px-3 py-1.5 text-sm cursor-pointer hover:bg-background focus:bg-background focus:outline-none"
            >
              {o.label}
            </DropdownMenu.Item>
          ))}
        </PillMenu>
      )}

      {availableFilters.includes("location") && (
        <LocationPill
          current={currentLocation}
          // `bbox: null` because location and bbox are conceptually
          // linked: the bbox in the URL is the viewport hint that
          // belongs to the *current* location (set by the city pivot
          // or by panning inside the filter). Changing or clearing
          // the location means the bbox is now stale — leaving it
          // would make the server-side map fetch keep spatially
          // clipping pins to the old city, and the camera would
          // refit to that local subset instead of the new global
          // (or new-city) result.
          onSubmit={(v) => push({ location: v || null, bbox: null })}
        />
      )}

      {hasAnyFilter(searchParams, availableFilters) && (
        <button
          type="button"
          onClick={() => {
            // Remove every filter the bar owns; leave unrelated params (e.g. `q`) alone.
            const clear: Record<string, null> = {};
            for (const k of [
              "medium",
              "price",
              "price_min",
              "price_max",
              "availability",
              ...(availableFilters.includes("location")
                ? // bbox rides with location — see LocationPill above.
                  ["location", "bbox"]
                : []),
            ]) {
              clear[k] = null;
            }
            push(clear);
          }}
          className="text-xs text-muted hover:text-foreground underline underline-offset-2"
        >
          Clear filters
        </button>
      )}
    </div>
  );
}

// ─────────────────────────────────────────────────────────────────────────────
// Subparts
// ─────────────────────────────────────────────────────────────────────────────

function PillMenu({
  label,
  active,
  onClear,
  children,
}: {
  label: string;
  active: boolean;
  onClear: () => void;
  children: React.ReactNode;
}) {
  // Wrapper is a styled <div>, not a <button>, so the "clear" button
  // can sit as a sibling. Original shape (button-inside-button) was
  // invalid HTML — browsers swallowed the inner button's click event,
  // which is why the × didn't actually clear the filter.
  return (
    <DropdownMenu.Root>
      <div
        className={clsx(
          "inline-flex items-center border text-sm transition-colors",
          active
            ? "border-foreground bg-foreground text-background"
            : "border-border bg-surface hover:bg-background"
        )}
      >
        <DropdownMenu.Trigger asChild>
          <button
            type="button"
            aria-pressed={active}
            className="px-3 py-1.5"
          >
            {label}
          </button>
        </DropdownMenu.Trigger>
        {active && (
          <button
            type="button"
            aria-label="Clear filter"
            onClick={onClear}
            className="pr-3 pl-1 py-1.5 hover:opacity-80"
          >
            ×
          </button>
        )}
      </div>
      <DropdownMenu.Portal>
        <DropdownMenu.Content
          align="start"
          sideOffset={6}
          className="bg-surface border border-border py-1 min-w-[12rem] z-50 shadow-lg max-h-72 overflow-y-auto"
        >
          {children}
        </DropdownMenu.Content>
      </DropdownMenu.Portal>
    </DropdownMenu.Root>
  );
}

/** Free-text location input — distinct from the dropdown pills because
 * locations are open-ended (any city name). Renders as a pill that
 * expands into an input on focus. */
function LocationPill({
  current,
  onSubmit,
}: {
  current?: string;
  onSubmit: (next: string) => void;
}) {
  const [editing, setEditing] = useState(false);
  const [draft, setDraft] = useState(current ?? "");
  const inputRef = useRef<HTMLInputElement>(null);

  useEffect(() => {
    if (editing) inputRef.current?.focus();
  }, [editing]);

  if (!editing) {
    // Wrapper div + sibling buttons — same shape as PillMenu so the
    // clear × actually fires (a nested button-in-button would have
    // its inner click swallowed by the browser).
    return (
      <div
        className={clsx(
          "inline-flex items-center border text-sm",
          current
            ? "border-foreground bg-foreground text-background"
            : "border-border bg-surface hover:bg-background"
        )}
      >
        <button
          type="button"
          aria-pressed={Boolean(current)}
          onClick={() => {
            setDraft(current ?? "");
            setEditing(true);
          }}
          className="px-3 py-1.5"
        >
          {current ? `Location: ${current}` : "Location"}
        </button>
        {current && (
          <button
            type="button"
            aria-label="Clear location"
            onClick={() => onSubmit("")}
            className="pr-3 pl-1 py-1.5 hover:opacity-80"
          >
            ×
          </button>
        )}
      </div>
    );
  }

  return (
    <form
      onSubmit={(e) => {
        e.preventDefault();
        onSubmit(draft.trim());
        setEditing(false);
      }}
      className="inline-flex items-center gap-1"
    >
      <input
        ref={inputRef}
        type="text"
        value={draft}
        onChange={(e) => setDraft(e.target.value)}
        onBlur={() => {
          // Submit on blur so clicking elsewhere applies the filter.
          if (draft.trim() !== (current ?? "").trim()) {
            onSubmit(draft.trim());
          }
          setEditing(false);
        }}
        placeholder="City or country"
        aria-label="Location"
        className="px-3 py-1.5 text-sm border border-foreground bg-surface focus:outline-none w-44"
      />
    </form>
  );
}

// ─────────────────────────────────────────────────────────────────────────────
// Helpers
// ─────────────────────────────────────────────────────────────────────────────

function numOrUndef(s: string | null): number | undefined {
  if (!s) return undefined;
  const n = Number(s);
  return Number.isFinite(n) ? n : undefined;
}

function hasAnyFilter(
  sp: URLSearchParams,
  available: FilterKind[]
): boolean {
  if (available.includes("medium") && sp.get("medium")) return true;
  if (available.includes("price") && (sp.get("price") || sp.get("price_min") || sp.get("price_max")))
    return true;
  if (available.includes("availability") && sp.get("availability")) return true;
  if (available.includes("location") && sp.get("location")) return true;
  return false;
}

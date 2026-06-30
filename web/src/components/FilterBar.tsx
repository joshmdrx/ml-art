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
  buildMediumParam,
  MEDIUM_CATEGORIES,
  parseMediumParam,
  PRICE_BUCKETS,
  priceParamsFromToken,
  SIZE_BANDS,
  type FilterKind,
} from "@/lib/filterBar";
import { mediumLabel } from "@/lib/medium";

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
  // T-073 — medium is now a multi-value comma-separated parameter
  // against the canonical taxonomy (`painting,print`). `parseMediumParam`
  // drops unknown tokens so a bookmarked URL with a renamed category
  // surfaces what survives instead of hard-erroring.
  const currentMedia = parseMediumParam(searchParams.get("medium"));
  const currentAvailability = searchParams.get("availability") || undefined;
  const currentLocation = searchParams.get("location") || undefined;
  const priceMin = numOrUndef(searchParams.get("price_min"));
  const priceMax = numOrUndef(searchParams.get("price_max"));
  const currentPriceToken = bucketTokenFromPriceParams(priceMin, priceMax);
  const currentSizeRaw = searchParams.get("size") || undefined;
  // Tolerant lookup — unknown tokens render as if the filter is unset
  // (the API does the same). Keeps a bookmarked `?size=xl` from
  // looking "active" forever after we ever rename a band.
  const currentSize = SIZE_BANDS.find((b) => b.token === currentSizeRaw);

  return (
    <div
      className="mb-6 flex flex-wrap items-center gap-2"
      role="toolbar"
      aria-label="Filters"
    >
      {availableFilters.includes("medium") && (
        <PillMenu
          label={
            currentMedia.length === 0
              ? "Medium"
              : currentMedia.length === 1
                ? `Medium: ${mediumLabel(currentMedia[0])}`
                : `Medium: ${currentMedia.length} selected`
          }
          active={currentMedia.length > 0}
          onClear={() => push({ medium: null })}
        >
          {/* T-073 — multi-select. Each item is a toggle: clicking
              flips that code in/out of the `?medium=` comma list.
              Radix's onSelect closes the menu by default, but here we
              want it to stay open while the artist picks more than
              one. Override the close-on-select with `e.preventDefault()`
              on the item. */}
          {MEDIUM_CATEGORIES.map((code) => {
            const isOn = currentMedia.includes(code);
            return (
              <DropdownMenu.Item
                key={code}
                onSelect={(e) => {
                  e.preventDefault();
                  const next = isOn
                    ? currentMedia.filter((c) => c !== code)
                    : [...currentMedia, code];
                  push({ medium: buildMediumParam(next) });
                }}
                className={clsx(
                  "px-3 py-1.5 text-sm cursor-pointer hover:bg-background focus:bg-background focus:outline-none flex items-center gap-2",
                )}
              >
                <span
                  aria-hidden
                  className={clsx(
                    "inline-block w-3 h-3 border",
                    isOn
                      ? "border-foreground bg-foreground"
                      : "border-border",
                  )}
                />
                {mediumLabel(code)}
              </DropdownMenu.Item>
            );
          })}
        </PillMenu>
      )}

      {availableFilters.includes("price") && (
        <PillMenu
          label={priceLabel(currentPriceToken, priceMin, priceMax)}
          active={
            Boolean(currentPriceToken) ||
            priceMin !== undefined ||
            priceMax !== undefined
          }
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
          <DropdownMenu.Separator className="my-1 border-t border-border" />
          {/* T-062 — custom range. Inputs are in major GBP units (£500),
              converted to minor (50000) when submitted to match the
              existing bucket scale. Submitting clears `price` (the
              bucket token) so the label switches to the custom range.
              The server-side filter operates on `price_gbp_cents`
              (T-080), so submitting numeric pounds works for artworks
              listed in any tracked currency. */}
          <CustomPriceRange
            initialMin={priceMin}
            initialMax={priceMax}
            onSubmit={(minMajor, maxMajor) => {
              push({
                price: null,
                price_min:
                  minMajor != null
                    ? Math.round(minMajor * 100).toString()
                    : null,
                price_max:
                  maxMajor != null
                    ? Math.round(maxMajor * 100).toString()
                    : null,
              });
            }}
          />
        </PillMenu>
      )}

      {availableFilters.includes("size") && (
        <PillMenu
          label={currentSize ? `Size: ${currentSize.label.split(" ")[0]}` : "Size"}
          active={Boolean(currentSize)}
          onClear={() => push({ size: null })}
        >
          {SIZE_BANDS.map((b) => (
            <DropdownMenu.Item
              key={b.token}
              onSelect={() => push({ size: b.token })}
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
              "size",
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

/** T-062 — custom min/max price inputs inside the price dropdown.
 *  Values are in major GBP units (£500, not 50000 pence) — the parent
 *  converts to minor units when submitting to the URL. Submitting
 *  with both fields blank clears the custom range. */
function CustomPriceRange({
  initialMin,
  initialMax,
  onSubmit,
}: {
  initialMin: number | undefined;
  initialMax: number | undefined;
  onSubmit: (minMajor: number | null, maxMajor: number | null) => void;
}) {
  const [minDraft, setMinDraft] = useState<string>(
    initialMin != null ? String(initialMin / 100) : "",
  );
  const [maxDraft, setMaxDraft] = useState<string>(
    initialMax != null ? String(initialMax / 100) : "",
  );

  const apply = () => {
    const parse = (s: string): number | null => {
      const trimmed = s.trim();
      if (trimmed === "") return null;
      const n = Number(trimmed);
      if (!Number.isFinite(n) || n < 0) return null;
      return n;
    };
    onSubmit(parse(minDraft), parse(maxDraft));
  };

  return (
    <div
      className="px-3 py-2"
      // Stop click + keydown from bubbling to the DropdownMenu, which
      // would otherwise close on each interaction.
      onClick={(e) => e.stopPropagation()}
      onKeyDown={(e) => e.stopPropagation()}
    >
      <p className="text-xs text-muted mb-1.5">Custom range</p>
      <form
        onSubmit={(e) => {
          e.preventDefault();
          apply();
        }}
        className="flex items-center gap-1"
      >
        <input
          type="number"
          inputMode="numeric"
          min={0}
          step={1}
          value={minDraft}
          onChange={(e) => setMinDraft(e.target.value)}
          placeholder="Min"
          aria-label="Minimum price"
          className="w-20 px-2 py-1 text-sm border border-border bg-background focus:outline-none focus:border-foreground"
        />
        <span className="text-muted">–</span>
        <input
          type="number"
          inputMode="numeric"
          min={0}
          step={1}
          value={maxDraft}
          onChange={(e) => setMaxDraft(e.target.value)}
          placeholder="Max"
          aria-label="Maximum price"
          className="w-20 px-2 py-1 text-sm border border-border bg-background focus:outline-none focus:border-foreground"
        />
        <button
          type="submit"
          className="ml-1 px-2 py-1 text-xs border border-foreground bg-foreground text-background"
        >
          Apply
        </button>
      </form>
    </div>
  );
}

/** Build the price pill's label, handling preset buckets + custom
 *  ranges + the empty default. */
function priceLabel(
  token: string | undefined,
  priceMin: number | undefined,
  priceMax: number | undefined,
): string {
  if (token) {
    const bucket = PRICE_BUCKETS.find((b) => b.token === token);
    if (bucket) return `Price: ${bucket.label}`;
  }
  const hasMin = priceMin !== undefined;
  const hasMax = priceMax !== undefined;
  if (!hasMin && !hasMax) return "Price";
  const fmt = (pence: number) => `£${Math.round(pence / 100).toLocaleString()}`;
  if (hasMin && hasMax) return `Price: ${fmt(priceMin!)}–${fmt(priceMax!)}`;
  if (hasMin) return `Price: ${fmt(priceMin!)}+`;
  return `Price: under ${fmt(priceMax!)}`;
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
  if (available.includes("size") && sp.get("size")) return true;
  return false;
}

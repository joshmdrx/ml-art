"use client";

/**
 * T-042 — city-pivot pills on `/search?map=1`.
 *
 * Horizontal strip of "London (12) · Berlin (8) · …" pills above the
 * map. Click → navigate to `/search?map=1&location=<city>&bbox=<city-bbox>`.
 * The `location` applies the same filter the FilterBar's location
 * facet writes (so grid + map narrow in sync, and the user gets a
 * clearable "Showing in London ✕" pill via `<FilterPill>`). The
 * `bbox` is set in the same hop so the camera actually flies to the
 * picked city — otherwise the filter would narrow the pins but
 * leave the viewport where it was.
 *
 * Doubles as the cold-start "where do I start?" affordance — without
 * it the map opens at a blank world view.
 *
 * When a location filter is already active we hide the strip: the
 * pill above is the affordance for changing or clearing it, and
 * leaving the strip up would invite ambiguous "London + Berlin"
 * mental models we don't actually support.
 *
 * Scale: the strip only shows the top {@link VISIBLE_LIMIT} cities
 * inline (ordered by count desc). The overflow lands in a popover
 * behind a "+N more" button so the chrome doesn't expand without
 * bound as the corpus grows. Past a few hundred cities we'd want a
 * proper geographic geocoder (typeahead + region grouping); this
 * keeps the v1 surface tidy without that machinery.
 *
 * Implementation note: we use `<Link>` rather than client-side state
 * mutation so the URL is the single source of truth — bookmark a
 * city, share the URL, hit back to undo. The server renders the new
 * grid + map together.
 */

import Link from "next/link";
import { useSearchParams } from "next/navigation";
import { useEffect, useRef, useState } from "react";
import type { CityPivot } from "@/lib/api";
import { searchHref } from "@/lib/searchMap/url";

interface Props {
  cities: CityPivot[];
}

/** How many city pills to show inline. The rest land in a popover.
 * 6 is enough for the visual rhythm of the toolbar at common screen
 * widths without crowding the filters above it. */
const VISIBLE_LIMIT = 6;

/** Pad a single-point bbox so fitBounds doesn't slam us to street
 * level for a one-pin city. 0.05° ≈ 5km at temperate latitudes —
 * a city-wide viewport even when the city has just one venue. */
const MIN_BBOX_HALF_DEG = 0.05;

function bboxFor(c: CityPivot): string {
  let { west, south, east, north } = c;
  if (east - west < MIN_BBOX_HALF_DEG * 2) {
    const cx = (west + east) / 2;
    west = cx - MIN_BBOX_HALF_DEG;
    east = cx + MIN_BBOX_HALF_DEG;
  }
  if (north - south < MIN_BBOX_HALF_DEG * 2) {
    const cy = (south + north) / 2;
    south = cy - MIN_BBOX_HALF_DEG;
    north = cy + MIN_BBOX_HALF_DEG;
  }
  return `${west.toFixed(4)},${south.toFixed(4)},${east.toFixed(4)},${north.toFixed(4)}`;
}

export function CityPivotStrip({ cities }: Props) {
  const searchParams = useSearchParams();
  const [overflowOpen, setOverflowOpen] = useState(false);
  const overflowRef = useRef<HTMLLIElement | null>(null);

  // Close the overflow popover on outside-click + Escape — standard
  // dismiss affordances. Mount only when actually open to keep the
  // event listener footprint trivial.
  useEffect(() => {
    if (!overflowOpen) return;
    function onPointer(e: MouseEvent) {
      if (
        overflowRef.current &&
        !overflowRef.current.contains(e.target as Node)
      ) {
        setOverflowOpen(false);
      }
    }
    function onKey(e: KeyboardEvent) {
      if (e.key === "Escape") setOverflowOpen(false);
    }
    document.addEventListener("mousedown", onPointer);
    document.addEventListener("keydown", onKey);
    return () => {
      document.removeEventListener("mousedown", onPointer);
      document.removeEventListener("keydown", onKey);
    };
  }, [overflowOpen]);

  if (cities.length === 0) return null;
  // The filter pill above is now the affordance for the active
  // location — don't double up the surface area.
  if ((searchParams.get("location") ?? "").trim().length > 0) return null;

  function hrefFor(c: CityPivot): string {
    // Pivoting is a filter action: set `location` (narrows grid +
    // map) AND set `bbox` to the city's own viewport (so the map
    // actually flies to the city — without this the filter would
    // narrow the pins but leave the camera wherever it was, e.g.
    // panned out over Melbourne when the user picked New York).
    // bbox and location are now consistent — no stale-viewport risk.
    const base = Object.fromEntries(searchParams.entries());
    return searchHref(base, {
      set: { map: "1", location: c.city, bbox: bboxFor(c) },
    });
  }

  const visible = cities.slice(0, VISIBLE_LIMIT);
  const overflow = cities.slice(VISIBLE_LIMIT);

  return (
    <nav
      aria-label="Jump to a city"
      className="mb-4 -mx-1 overflow-x-auto whitespace-nowrap"
    >
      <ul className="inline-flex gap-2 px-1">
        {visible.map((c) => (
          <li key={`${c.city}:${c.country ?? ""}`}>
            <Link
              href={hrefFor(c)}
              className="inline-flex items-baseline gap-1.5 border border-border bg-surface hover:bg-fg/10 px-3 py-1.5 text-sm"
            >
              <span>{c.city}</span>
              <span className="text-xs text-muted">({c.count})</span>
            </Link>
          </li>
        ))}
        {overflow.length > 0 && (
          <li ref={overflowRef} className="relative">
            <button
              type="button"
              aria-haspopup="menu"
              aria-expanded={overflowOpen}
              onClick={() => setOverflowOpen((v) => !v)}
              className="inline-flex items-baseline gap-1 border border-border bg-surface hover:bg-fg/10 px-3 py-1.5 text-sm"
            >
              <span>+ {overflow.length} more</span>
            </button>
            {overflowOpen && (
              <div
                role="menu"
                className="absolute left-0 top-full mt-1 z-30 min-w-[14rem] max-h-72 overflow-y-auto bg-surface border border-border shadow-lg py-1"
              >
                {overflow.map((c) => (
                  <Link
                    key={`${c.city}:${c.country ?? ""}`}
                    href={hrefFor(c)}
                    role="menuitem"
                    onClick={() => setOverflowOpen(false)}
                    className="flex items-baseline justify-between gap-3 px-3 py-1.5 text-sm hover:bg-background"
                  >
                    <span>{c.city}</span>
                    <span className="text-xs text-muted">({c.count})</span>
                  </Link>
                ))}
              </div>
            )}
          </li>
        )}
      </ul>
    </nav>
  );
}

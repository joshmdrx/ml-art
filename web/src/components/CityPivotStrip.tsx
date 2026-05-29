"use client";

/**
 * T-042 — city-pivot pills on `/search?map=1`.
 *
 * Horizontal strip of "London (12) · Berlin (8) · …" pills above the
 * map. Click → navigate to `/search?map=1&bbox=<city-bbox>` which
 * causes `SearchMap` to refetch and fitBounds to that city. Doubles
 * as the cold-start "where do I start?" affordance — without it the
 * map opens at a blank world view.
 *
 * Implementation note: we use `<Link>` rather than client-side bounds
 * mutation so the URL is the single source of truth — bookmark a
 * city, share the URL, hit back to undo. The `SearchMap` component
 * already reads `bbox` from the URL on every render.
 *
 * Server-rendered list isn't viable because we need to preserve the
 * other query params (filters, artist, etc.) at click time —
 * easier as a client component with `useSearchParams`.
 */

import Link from "next/link";
import { useSearchParams } from "next/navigation";
import type { CityPivot } from "@/lib/api";

interface Props {
  cities: CityPivot[];
}

/** Pad a single-point bbox so fitBounds doesn't zoom to street level.
 * 0.05° ≈ 5km at temperate latitudes; gives a city-wide viewport
 * even when there's one pin. */
const MIN_BBOX_HALF_DEG = 0.05;

export function CityPivotStrip({ cities }: Props) {
  const searchParams = useSearchParams();
  if (cities.length === 0) return null;

  function bboxFor(c: CityPivot): string {
    let { west, south, east, north } = c;
    // Pad a degenerate bbox (single pin) so we don't end up at zoom 22.
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

  function hrefFor(c: CityPivot): string {
    const usp = new URLSearchParams(searchParams.toString());
    usp.set("map", "1");
    usp.set("bbox", bboxFor(c));
    return `/search?${usp.toString()}`;
  }

  return (
    <nav
      aria-label="Jump to a city"
      className="mb-4 -mx-1 overflow-x-auto whitespace-nowrap"
    >
      <ul className="inline-flex gap-2 px-1">
        {cities.map((c) => (
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
      </ul>
    </nav>
  );
}

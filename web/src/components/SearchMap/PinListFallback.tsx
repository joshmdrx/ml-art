"use client";

/**
 * No-Mapbox-token fallback for the search map. Renders the pins as a
 * grid of cards instead of a real map. Same data shape as the
 * Mapbox-rendered surface — keeps the page useful in environments
 * (CI, restricted browsers) where the map can't load.
 */

import type { MapPin } from "@/lib/api";

export function PinListFallback({ pins }: { pins: MapPin[] }) {
  if (pins.length === 0) {
    return (
      <div className="py-24 text-center text-muted">
        No venues match this search.
      </div>
    );
  }
  return (
    <ul className="grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-3 gap-4">
      {pins.map((p) => (
        <li key={p.location_id} className="border border-border bg-surface p-4">
          <p className="text-[10px] uppercase tracking-wider text-muted">
            {p.kind}
            {p.city ? ` · ${p.city}` : ""}
          </p>
          <p className="mt-1 font-medium">{p.name}</p>
          <p className="text-sm text-muted">
            Showing{" "}
            <a
              href={`/artists/${encodeURIComponent(p.artist.slug)}`}
              className="underline"
            >
              {p.artist.display_name}
            </a>
          </p>
        </li>
      ))}
    </ul>
  );
}

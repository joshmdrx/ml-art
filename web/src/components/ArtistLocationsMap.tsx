"use client";

/**
 * T-038 G4 — Map widget on `/artists/[slug]`.
 *
 * Renders one pin per geocoded `artist_locations` row. Click a pin →
 * popover with name, address, optional website link, and a "Listed by
 * the artist" disclosure (the v1 trust model; see `decisions.md`
 * 2026-05-28).
 *
 * Three render paths:
 *  - `locations.length === 0` → caller renders a fallback "based in
 *    {city}" pill instead; this component returns `null`
 *  - No `NEXT_PUBLIC_MAPBOX_TOKEN` → degrades to a non-interactive list
 *    of pin cards (same data, no map). Keeps the page from blowing up
 *    in local dev without the paid key
 *  - Token present → real Mapbox GL JS map
 *
 * Mapbox GL is heavy (~250KB gzipped). We import it dynamically inside
 * the effect so it doesn't ship in the initial JS bundle for artist
 * pages that have zero locations (which is most of them today).
 */

import { useEffect, useRef, useState } from "react";
import Link from "next/link";
import type { PublicArtistLocation } from "@/lib/api";
import { reportError } from "@/lib/reportError";

interface Props {
  /** Optional so we don't crash when an older API build (or a cached
   * payload) omits the `locations` field. Treated as `[]` in that
   * case — same behavior as an artist with no locations. */
  locations?: PublicArtistLocation[];
  /** Artist slug, used to build the "See on map" CTA that opens the
   * full search-map view filtered to this artist (T-041). */
  artistSlug: string;
}

const MAPBOX_TOKEN = process.env.NEXT_PUBLIC_MAPBOX_TOKEN;

export function ArtistLocationsMap({ locations, artistSlug }: Props) {
  const safe = locations ?? [];
  if (safe.length === 0) return null;

  if (!MAPBOX_TOKEN) {
    return <LocationsListFallback locations={safe} artistSlug={artistSlug} />;
  }

  return <MapboxMap locations={safe} artistSlug={artistSlug} />;
}

// ─────────────────────────────────────────────────────────────────────────────
// Map (only mounted when there's a token)
// ─────────────────────────────────────────────────────────────────────────────

function MapboxMap({
  locations,
  artistSlug,
}: {
  locations: PublicArtistLocation[];
  artistSlug: string;
}) {
  const containerRef = useRef<HTMLDivElement | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let cancelled = false;
    let map: import("mapbox-gl").Map | null = null;

    (async () => {
      try {
        // Dynamic import keeps mapbox-gl out of the server bundle AND
        // out of the initial client bundle for pages that don't render
        // this component.
        const mapboxgl = (await import("mapbox-gl")).default;
        await import("mapbox-gl/dist/mapbox-gl.css");
        if (cancelled || !containerRef.current) return;

        mapboxgl.accessToken = MAPBOX_TOKEN!;

        // Compute initial bounds from the locations. Single pin: center
        // and zoom in. Multiple pins: fitBounds so they all show.
        const center: [number, number] = [
          locations[0].lng,
          locations[0].lat,
        ];

        map = new mapboxgl.Map({
          container: containerRef.current,
          style: "mapbox://styles/mapbox/light-v11",
          center,
          zoom: 13,
          // Lightly interactive — drag/scroll is fine; rotating + tilting
          // adds nothing on a profile-card-sized embed.
          pitchWithRotate: false,
          dragRotate: false,
        });

        map.addControl(new mapboxgl.NavigationControl({ showCompass: false }));

        for (const loc of locations) {
          const popupHtml = renderPopupHtml(loc);
          const popup = new mapboxgl.Popup({
            offset: 18,
            closeButton: true,
            closeOnClick: true,
          }).setHTML(popupHtml);

          new mapboxgl.Marker({ color: "#222" })
            .setLngLat([loc.lng, loc.lat])
            .setPopup(popup)
            .addTo(map);
        }

        if (locations.length > 1) {
          const bounds = new mapboxgl.LngLatBounds();
          for (const l of locations) bounds.extend([l.lng, l.lat]);
          map.fitBounds(bounds, { padding: 60, maxZoom: 14, duration: 0 });
        }
      } catch (e) {
        reportError(e, { surface: "artist-locations-map" });
        if (!cancelled) {
          setError("Couldn't load the map. Showing the list instead.");
        }
      }
    })();

    return () => {
      cancelled = true;
      map?.remove();
    };
  }, [locations]);

  if (error) {
    return (
      <LocationsListFallback
        locations={locations}
        artistSlug={artistSlug}
        note={error}
      />
    );
  }

  return (
    <section className="mb-12 md:mb-16">
      <div className="flex items-baseline justify-between mb-3">
        <h2 className="font-serif text-xl">Where to see this work</h2>
        <Link
          href={`/search?map=1&artist=${encodeURIComponent(artistSlug)}`}
          className="text-sm underline text-muted hover:text-foreground"
        >
          See on full map →
        </Link>
      </div>
      <div
        ref={containerRef}
        role="region"
        aria-label="Map of locations where this artist's work can be seen"
        className="h-64 md:h-80 w-full border border-border bg-surface"
      />
      <p className="mt-2 text-xs text-muted">
        Listed by the artist.
      </p>
    </section>
  );
}

/** Returns a sanitized HTML string for a Mapbox popup. We assemble it
 * by hand because Mapbox popups accept HTML strings, not React. Each
 * field is escaped via `escapeHtml`. */
function renderPopupHtml(loc: PublicArtistLocation): string {
  const kindLabel = loc.kind === "gallery" ? "Gallery" : "Studio";
  const linkLine = loc.website_url
    ? `<a href="${escapeAttr(loc.website_url)}" target="_blank" rel="noopener noreferrer" style="text-decoration: underline; display: block; margin-top: 6px;">Visit website ↗</a>`
    : "";

  return `
    <div style="font-family: inherit; min-width: 180px; max-width: 260px;">
      <p style="font-size: 10px; letter-spacing: 0.08em; text-transform: uppercase; color: #666; margin: 0;">${kindLabel}</p>
      <p style="font-weight: 600; margin: 4px 0 2px;">${escapeHtml(loc.name)}</p>
      <p style="font-size: 13px; color: #444; margin: 0;">${escapeHtml(loc.address)}</p>
      ${linkLine}
      <p style="font-size: 11px; color: #888; margin-top: 8px;">Listed by the artist.</p>
    </div>
  `;
}

function escapeHtml(s: string): string {
  return s
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;")
    .replace(/"/g, "&quot;")
    .replace(/'/g, "&#39;");
}

function escapeAttr(s: string): string {
  return escapeHtml(s);
}

// ─────────────────────────────────────────────────────────────────────────────
// Fallback list — used when MAPBOX_TOKEN is absent OR the map errors out
// ─────────────────────────────────────────────────────────────────────────────

function LocationsListFallback({
  locations,
  artistSlug,
  note,
}: {
  locations: PublicArtistLocation[];
  artistSlug: string;
  note?: string;
}) {
  return (
    <section className="mb-12 md:mb-16">
      <div className="flex items-baseline justify-between mb-3">
        <h2 className="font-serif text-xl">Where to see this work</h2>
        <Link
          href={`/search?map=1&artist=${encodeURIComponent(artistSlug)}`}
          className="text-sm underline text-muted hover:text-foreground"
        >
          See on full map →
        </Link>
      </div>
      {note && <p className="mb-3 text-xs text-amber-700">{note}</p>}
      <ul className="divide-y divide-border border border-border bg-surface">
        {locations.map((loc) => (
          <li key={loc.id} className="p-4">
            <p className="text-[10px] uppercase tracking-wider text-muted">
              {loc.kind}
            </p>
            <p className="mt-1 font-medium">{loc.name}</p>
            <p className="text-sm text-muted">{loc.address}</p>
            {loc.website_url && (
              <a
                href={loc.website_url}
                target="_blank"
                rel="noopener noreferrer"
                className="mt-1 inline-block text-sm underline"
              >
                Visit website ↗
              </a>
            )}
          </li>
        ))}
      </ul>
      <p className="mt-2 text-xs text-muted">Listed by the artist.</p>
    </section>
  );
}

"use client";

/**
 * Popup content for the search map — rendered as real React DOM via
 * Mapbox's `Popup.setDOMContent`, not interpolated HTML strings.
 *
 * The previous version hand-built HTML with `escapeHtml(...)` and
 * shipped it through `setHTML`. That worked but:
 *   - One missed escape was an XSS in user-controlled content.
 *   - Links did full page-reloads instead of Next-Link client-side
 *     navigation, breaking the back-button + losing scroll position.
 *   - There was no design system involvement; everything was inline
 *     styles for parity with Mapbox's CSS reset inside the popup.
 *
 * Rendering with `createRoot` per popup keeps the React surface
 * intact: components escape by default, `<Link>` works, Tailwind
 * classes apply. A small `mountPopupContent` helper wraps the
 * createRoot/unmount lifecycle so callers don't have to reason
 * about it.
 */

import { createRoot, type Root } from "react-dom/client";
import Link from "next/link";

import type { PinProperties } from "@/lib/searchMap/geojson";

/** Single-pin popup body. */
export function PinPopup({ props }: { props: PinProperties }) {
  const kindLabel = props.kind === "gallery" ? "Gallery" : "Studio";
  return (
    <div className="font-sans min-w-[200px] max-w-[240px]">
      {props.artist_image && (
        // eslint-disable-next-line @next/next/no-img-element
        <img
          src={props.artist_image}
          alt=""
          className="w-full h-24 object-cover mb-2"
        />
      )}
      <p className="text-[10px] tracking-wider uppercase text-muted m-0">
        {kindLabel}
        {props.address_city ? ` · ${props.address_city}` : ""}
      </p>
      <p className="font-semibold mt-1 mb-0">{props.name}</p>
      <p className="text-sm text-foreground/70 my-0.5">
        Showing {props.artist_name}
      </p>
      <Link
        href={`/artists/${encodeURIComponent(props.artist_slug)}`}
        className="inline-block mt-1.5 text-sm underline"
      >
        View portfolio →
      </Link>
    </div>
  );
}

/**
 * Popup shown when the user clicks a sidebar card and we can't
 * locate the artist's pin in the current pin set. Two cases land
 * here today:
 *
 *   1. The artist genuinely hasn't shared an `artist_locations` row
 *      — there's nothing for us to fly to, ever.
 *   2. The artist *has* a location but their pin wasn't loaded
 *      because they only appeared after a Load-more page — the map
 *      endpoint was called with the first page's artist_ids before
 *      the user paginated. (Tracking that as a follow-up under T-037
 *      / T-045 — refetch pins when grid pages grow.)
 *
 * The client can't tell these two apart from where it sits, so the
 * copy stays neutral: no assertion that the artist is unmapped.
 * The user gets the artwork's image (familiar context, since they
 * just clicked it) + a clear portfolio link.
 *
 * Same visual skin as `<PinPopup>` so the click contract feels
 * consistent — every card click surfaces info about its artist.
 */
export function UnmappedArtistPopup({
  artistSlug,
  artistName,
  imageUrl,
}: {
  artistSlug: string;
  artistName: string;
  imageUrl: string | null;
}) {
  return (
    <div className="font-sans min-w-[200px] max-w-[240px]">
      {imageUrl && (
        // eslint-disable-next-line @next/next/no-img-element
        <img src={imageUrl} alt="" className="w-full h-24 object-cover mb-2" />
      )}
      <p className="font-semibold mt-1 mb-0">{artistName}</p>
      <Link
        href={`/artists/${encodeURIComponent(artistSlug)}`}
        className="inline-block mt-1.5 text-sm underline"
      >
        View portfolio →
      </Link>
    </div>
  );
}

/** Cluster-of-coincident-pins popup body — one row per venue. */
export function ClusterListPopup({
  leaves,
}: {
  leaves: GeoJSON.Feature<GeoJSON.Geometry, PinProperties>[];
}) {
  return (
    <div className="font-sans min-w-[220px] max-w-[280px]">
      <p className="text-[11px] tracking-wider uppercase text-muted m-0 mb-1">
        {leaves.length} venues at this location
      </p>
      <ul className="list-none p-0 m-0 max-h-80 overflow-y-auto">
        {leaves.map((leaf, i) => {
          const p = leaf.properties;
          const kindLabel = p.kind === "gallery" ? "Gallery" : "Studio";
          return (
            <li key={`${p.location_id}:${i}`} className="py-2 border-t border-border first:border-t-0">
              <p className="text-[10px] tracking-wider uppercase text-muted m-0">
                {kindLabel}
              </p>
              <p className="font-semibold mt-0.5 mb-0">{p.name}</p>
              <Link
                href={`/artists/${encodeURIComponent(p.artist_slug)}`}
                className="text-sm underline"
              >
                {p.artist_name} →
              </Link>
            </li>
          );
        })}
      </ul>
    </div>
  );
}

/**
 * Render React content into a freshly-created host element so
 * Mapbox's `Popup.setDOMContent` can take ownership of it. Returns
 * the element + a cleanup callback the caller should run when the
 * popup closes (Mapbox emits a `close` event on the Popup).
 */
export function mountPopupContent(content: React.ReactNode): {
  element: HTMLDivElement;
  unmount: () => void;
} {
  const element = document.createElement("div");
  const root: Root = createRoot(element);
  root.render(content);
  return {
    element,
    // Unmount asynchronously — Mapbox can fire the `close` event
    // mid-render, and unmount-during-render throws.
    unmount: () => {
      setTimeout(() => root.unmount(), 0);
    },
  };
}

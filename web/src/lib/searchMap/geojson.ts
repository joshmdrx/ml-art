/**
 * MapPin → GeoJSON FeatureCollection for the Mapbox `pins` source.
 *
 * Lives separately from the SearchMap component so the property
 * shape we hand to Mapbox is documented + diffable, and the popup
 * renderers can lean on a single source of truth for what's in
 * `feature.properties`.
 */

import type { MapPin } from "@/lib/api";

/**
 * Shape of `feature.properties` for every pin we insert. Mapbox
 * flattens nested objects in its property bag, so we flatten the
 * `artist` sub-record at insert time and the popup readers see this
 * exact shape on the other side.
 */
export interface PinProperties {
  location_id: string;
  name: string;
  kind: string;
  /** May be empty string (Mapbox doesn't serialise undefined cleanly). */
  address_city: string;
  artist_slug: string;
  artist_name: string;
  /** May be empty string. */
  artist_image: string;
}

export function toFeatureCollection(
  pins: MapPin[]
): GeoJSON.FeatureCollection<GeoJSON.Point, PinProperties> {
  return {
    type: "FeatureCollection",
    features: pins.map((p) => ({
      type: "Feature",
      // Top-level `id` so Mapbox's `setFeatureState` can address the
      // feature directly (used by the card-hover → pin-highlight
      // sync, T-045 L2). Without an id, feature-state can only key
      // off `properties.cluster_id` for cluster features.
      id: p.location_id,
      geometry: {
        type: "Point",
        coordinates: [p.lng, p.lat],
      },
      properties: {
        location_id: p.location_id,
        name: p.name,
        kind: p.kind,
        address_city: p.city ?? "",
        artist_slug: p.artist.slug,
        artist_name: p.artist.display_name,
        artist_image: p.artist.primary_image_url ?? "",
      },
    })),
  };
}

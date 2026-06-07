/**
 * Named constants for the `/search?map=1` surface.
 *
 * The map surface picked up a pile of magic numbers across multiple
 * files — debounce timings, geographic epsilons, layout offsets, etc.
 * Centralising them here means anyone tuning the map can see the full
 * knob panel in one place, with a one-line rationale per knob.
 */

/**
 * Debounce window between Mapbox's `moveend` and our `/v1/search/map`
 * refetch. Set to feel responsive on a single pan but avoid hammering
 * the API during an inertial scroll or zoom drag (which can emit
 * many moveends in quick succession).
 */
export const MOVEEND_DEBOUNCE_MS = 300;

/**
 * Mapbox GeoJSON source clustering knobs. `clusterRadius` is in
 * screen pixels; `clusterMaxZoom` is the max zoom at which we
 * still cluster (above this everything renders as individual pins).
 */
export const CLUSTER_RADIUS_PX = 50;
export const CLUSTER_MAX_ZOOM = 14;

/**
 * Coincidence tolerance for "are these pins effectively at the same
 * spot?" Used by the cluster-click handler to decide whether to zoom
 * in or open a list popup. 1e-4 degrees ≈ 11m at the equator — wide
 * enough to absorb the demo seed (every gallery anchored on the same
 * city centroid) and real galleries on the same building address.
 */
export const COINCIDENT_EPSILON_DEG = 1e-4;

/**
 * Top-right offset for the Near-me overlay button inside the map.
 * Pushes it below Mapbox's stock `NavigationControl` (which takes
 * ~58–64px depending on whether the compass is shown). We omit the
 * compass, so 64px clears it with a touch of breathing room.
 */
export const NEAR_ME_TOP_OFFSET_PX = 64;

/**
 * How far the world view spans on initial render when we have no
 * pins to fit to. `[0, 30]` lat/lng centers near the equator at a
 * latitude that shows both hemispheres at a comfortable framing.
 */
export const WORLD_VIEW_CENTER: [number, number] = [0, 30];
export const WORLD_VIEW_ZOOM = 1.4;

/**
 * Padding applied to fitBounds calls (in pixels). 40px when bbox is
 * supplied via the URL, 60px when fitting to a fresh pin set so the
 * outermost pins aren't flush with the viewport edge.
 */
export const FIT_BOUNDS_URL_PADDING = 40;
export const FIT_BOUNDS_PINS_PADDING = 60;

/**
 * Cap on how many features `getClusterLeaves` should return. The
 * supercluster docs note no hard ceiling, but we cap at 50 for
 * popup readability — any coincident city cluster with >50 venues
 * is a UX problem we'd want to solve differently anyway.
 */
export const CLUSTER_LEAVES_LIMIT = 50;

/**
 * Unit tests for the pure helpers extracted from SearchMap. None of
 * these touch Mapbox; they're testable bbox / coincidence / format
 * checks that previously lived inline in the React component and
 * had to be exercised through full E2E to catch a regression.
 */

import { describe, expect, it } from "vitest";

import {
  bboxesApproxEqual,
  bboxToString,
  clampBbox,
  parseBboxString,
} from "@/lib/searchMap/bbox";
import { leavesAreCoincident, pointCoords } from "@/lib/searchMap/cluster";
import { COINCIDENT_EPSILON_DEG } from "@/lib/searchMap/constants";
import { toFeatureCollection } from "@/lib/searchMap/geojson";

describe("clampBbox", () => {
  it("passes through legal bounds unchanged", () => {
    const b = { west: -10, south: 40, east: 10, north: 60 };
    expect(clampBbox(b)).toEqual(b);
  });

  it("clamps over-wide longitude to ±180", () => {
    expect(clampBbox({ west: -500, south: 0, east: 500, north: 10 })).toEqual({
      west: -180,
      south: 0,
      east: 180,
      north: 10,
    });
  });

  it("clamps over-wide latitude to ±90", () => {
    expect(clampBbox({ west: 0, south: -200, east: 10, north: 300 })).toEqual({
      west: 0,
      south: -90,
      east: 10,
      north: 90,
    });
  });

  it("returns null when the clamped bbox has zero width", () => {
    // Both lngs clamp to 180 → zero-width.
    expect(clampBbox({ west: 200, south: 0, east: 300, north: 10 })).toBeNull();
  });

  it("returns null when the input is inverted", () => {
    expect(clampBbox({ west: 10, south: 0, east: -10, north: 10 })).toBeNull();
  });
});

describe("bboxToString / parseBboxString", () => {
  it("round-trips a typical bbox at 4-decimal precision", () => {
    const b = { west: -0.1278, south: 51.5074, east: 0.0, north: 51.6 };
    const s = bboxToString(b);
    expect(s).toBe("-0.1278,51.5074,0.0000,51.6000");
    expect(parseBboxString(s)).toEqual({
      west: -0.1278,
      south: 51.5074,
      east: 0.0,
      north: 51.6,
    });
  });

  it("parses returns null on malformed input", () => {
    expect(parseBboxString("not,a,bbox")).toBeNull();
    expect(parseBboxString("1,2,3")).toBeNull();
    expect(parseBboxString("1,2,3,nan")).toBeNull();
    expect(parseBboxString("")).toBeNull();
  });
});

describe("bboxesApproxEqual", () => {
  it("returns true for the same bbox", () => {
    const b = { west: -0.1, south: 51.5, east: 0.1, north: 51.6 };
    expect(bboxesApproxEqual(b, b)).toBe(true);
  });

  it("returns true for tiny differences (Mapbox round-trip rounding)", () => {
    // Typical drift we see when fitBounds settles slightly off the
    // requested target: a few thousandths of a degree.
    expect(
      bboxesApproxEqual(
        { west: -0.1, south: 51.5, east: 0.1, north: 51.6 },
        { west: -0.099, south: 51.501, east: 0.101, north: 51.6 }
      )
    ).toBe(true);
  });

  it("returns false when one corner moves meaningfully (different city)", () => {
    expect(
      bboxesApproxEqual(
        { west: -0.1, south: 51.5, east: 0.1, north: 51.6 }, // London
        { west: 13.3, south: 52.4, east: 13.5, north: 52.6 } // Berlin
      )
    ).toBe(false);
  });

  it("honors a custom tolerance", () => {
    const a = { west: 0, south: 0, east: 1, north: 1 };
    const b = { west: 0.05, south: 0, east: 1, north: 1 };
    expect(bboxesApproxEqual(a, b, 0.01)).toBe(false);
    expect(bboxesApproxEqual(a, b, 0.1)).toBe(true);
  });
});

describe("leavesAreCoincident", () => {
  function feat(lng: number, lat: number): GeoJSON.Feature<GeoJSON.Geometry> {
    return {
      type: "Feature",
      geometry: { type: "Point", coordinates: [lng, lat] },
      properties: {},
    };
  }

  it("returns false for a single feature", () => {
    expect(leavesAreCoincident([feat(0, 0)])).toBe(false);
  });

  it("returns true when all features share the exact lat/lng", () => {
    expect(
      leavesAreCoincident([feat(-1.08, 51.27), feat(-1.08, 51.27)])
    ).toBe(true);
  });

  it("returns true within the epsilon (~10m)", () => {
    const dx = COINCIDENT_EPSILON_DEG / 2;
    expect(
      leavesAreCoincident([
        feat(-1.08, 51.27),
        feat(-1.08 + dx, 51.27 - dx),
      ])
    ).toBe(true);
  });

  it("returns false when one feature is meaningfully apart", () => {
    expect(
      leavesAreCoincident([feat(-1.08, 51.27), feat(-0.5, 51.5)])
    ).toBe(false);
  });

  it("returns false for non-Point geometry", () => {
    const polyFeature: GeoJSON.Feature<GeoJSON.Geometry> = {
      type: "Feature",
      geometry: {
        type: "Polygon",
        coordinates: [
          [
            [0, 0],
            [1, 0],
            [1, 1],
            [0, 0],
          ],
        ],
      },
      properties: {},
    };
    expect(leavesAreCoincident([polyFeature, feat(0, 0)])).toBe(false);
  });
});

describe("pointCoords", () => {
  it("returns [lng, lat] for a Point", () => {
    expect(
      pointCoords({ type: "Point", coordinates: [-0.1278, 51.5074] })
    ).toEqual([-0.1278, 51.5074]);
  });

  it("returns null for a non-Point geometry", () => {
    expect(
      pointCoords({
        type: "Polygon",
        coordinates: [],
      })
    ).toBeNull();
  });
});

describe("toFeatureCollection", () => {
  it("flattens the artist sub-record into properties", () => {
    const fc = toFeatureCollection([
      {
        location_id: "loc-1",
        lat: 51,
        lng: -0.1,
        name: "Test Gallery",
        kind: "gallery",
        city: "London",
        country: "GB",
        artist: {
          slug: "alice-test",
          display_name: "Alice Test",
          primary_image_url: null,
        },
      },
    ]);
    expect(fc.features).toHaveLength(1);
    const props = fc.features[0].properties;
    expect(props.artist_slug).toBe("alice-test");
    expect(props.artist_name).toBe("Alice Test");
    // null → empty string (Mapbox property bag can't carry undefined cleanly).
    expect(props.artist_image).toBe("");
    expect(props.address_city).toBe("London");
  });

  it("uses GeoJSON [lng, lat] order, not [lat, lng]", () => {
    const fc = toFeatureCollection([
      {
        location_id: "loc-1",
        lat: 51,
        lng: -0.1,
        name: "X",
        kind: "studio",
        city: null,
        country: null,
        artist: {
          slug: "x",
          display_name: "X",
          primary_image_url: null,
        },
      },
    ]);
    expect(fc.features[0].geometry.coordinates).toEqual([-0.1, 51]);
  });
});

import { describe, expect, it } from "vitest";
import {
  applyFilterParam,
  bucketTokenFromPriceParams,
  PRICE_BUCKETS,
  priceParamsFromToken,
} from "@/lib/filterBar";

describe("applyFilterParam", () => {
  it("sets a new key without disturbing existing ones", () => {
    const result = applyFilterParam(new URLSearchParams("q=ukiyo"), {
      medium: "Painting",
    });
    expect(new URLSearchParams(result).get("q")).toBe("ukiyo");
    expect(new URLSearchParams(result).get("medium")).toBe("Painting");
  });

  it("removes a key when set to null", () => {
    const result = applyFilterParam(
      new URLSearchParams("q=ukiyo&medium=Cubism"),
      { medium: null }
    );
    expect(new URLSearchParams(result).has("medium")).toBe(false);
    expect(new URLSearchParams(result).get("q")).toBe("ukiyo");
  });

  it("removes a key when set to empty string", () => {
    const result = applyFilterParam(
      new URLSearchParams("location=Berlin"),
      { location: "" }
    );
    expect(new URLSearchParams(result).has("location")).toBe(false);
  });

  it("handles multiple updates atomically", () => {
    const result = applyFilterParam(
      new URLSearchParams("q=blue&medium=Old"),
      { medium: "Sculpture", availability: "available" }
    );
    const usp = new URLSearchParams(result);
    expect(usp.get("medium")).toBe("Sculpture");
    expect(usp.get("availability")).toBe("available");
    expect(usp.get("q")).toBe("blue");
  });
});

describe("price bucket round-trip", () => {
  it("token → price params → token is stable", () => {
    for (const bucket of PRICE_BUCKETS) {
      const params = priceParamsFromToken(bucket.token);
      expect(params).not.toBeNull();
      const back = bucketTokenFromPriceParams(
        params?.price_min,
        params?.price_max
      );
      expect(back).toBe(bucket.token);
    }
  });

  it("unknown token returns null", () => {
    expect(priceParamsFromToken("not-a-bucket")).toBeNull();
    expect(priceParamsFromToken(null)).toBeNull();
    expect(priceParamsFromToken(undefined)).toBeNull();
  });

  it("price ranges that don't match a bucket exactly return undefined token", () => {
    // A custom range a hand-edited URL might carry — shouldn't masquerade
    // as a bucket selection.
    expect(bucketTokenFromPriceParams(123, 456)).toBeUndefined();
  });

  it("under-$500 bucket sets only price_max", () => {
    const p = priceParamsFromToken("u500");
    expect(p?.price_min).toBeUndefined();
    expect(p?.price_max).toBe(50_000);
  });

  it("$10k+ bucket sets only price_min", () => {
    const p = priceParamsFromToken("10kplus");
    expect(p?.price_min).toBe(1_000_000);
    expect(p?.price_max).toBeUndefined();
  });
});

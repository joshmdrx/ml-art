import { describe, expect, it } from "vitest";
import { formatPrice, formatDimensions } from "@/lib/api";

describe("formatPrice", () => {
  it("formats USD with $ and no decimals", () => {
    expect(formatPrice(120000, "USD")).toBe("$1,200");
  });

  it("formats EUR with the euro symbol", () => {
    // The exact glyph order depends on the runtime ICU table; we just
    // assert presence rather than exact prefix/suffix.
    const out = formatPrice(50000, "EUR");
    expect(out).not.toBeNull();
    expect(out).toMatch(/500/);
    expect(out).toMatch(/€/);
  });

  it("returns null when price_cents is null", () => {
    expect(formatPrice(null, "USD")).toBeNull();
  });

  it("falls back gracefully for non-ISO currencies", () => {
    // Anything that Intl rejects is wrapped with a digit-then-code form.
    const out = formatPrice(10000, "ZZZ");
    expect(out).toContain("100");
    expect(out).toContain("ZZZ");
  });

  it("handles zero cents", () => {
    expect(formatPrice(0, "USD")).toBe("$0");
  });
});

describe("formatDimensions", () => {
  it("returns null for null input", () => {
    expect(formatDimensions(null)).toBeNull();
  });

  it("returns null when both height and width are missing", () => {
    expect(formatDimensions({ unit: "cm" })).toBeNull();
  });

  it("formats height × width in cm by default", () => {
    expect(formatDimensions({ height: 60, width: 40 })).toBe("60 × 40 cm");
  });

  it("formats height × width × depth when depth present", () => {
    expect(formatDimensions({ height: 60, width: 40, depth: 5 })).toBe(
      "60 × 40 × 5 cm"
    );
  });

  it("honors the unit field when provided", () => {
    expect(formatDimensions({ height: 24, width: 36, unit: "in" })).toBe(
      "24 × 36 in"
    );
  });

  it("handles a single dimension (height only)", () => {
    expect(formatDimensions({ height: 100 })).toBe("100 cm");
  });
});

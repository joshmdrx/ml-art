import { describe, it, expect } from "vitest";
import {
  formatPriceForInput,
  minorUnitsFor,
  parsePrice,
} from "@/lib/parsePrice";

describe("parsePrice", () => {
  it("returns null for empty input", () => {
    expect(parsePrice("")).toBeNull();
    expect(parsePrice("   ")).toBeNull();
  });

  it("parses bare integer with fallback currency", () => {
    expect(parsePrice("120")).toEqual({ amount_minor: 12000, currency: "USD" });
    expect(parsePrice("120", "GBP")).toEqual({
      amount_minor: 12000,
      currency: "GBP",
    });
  });

  it("parses pound symbol", () => {
    expect(parsePrice("£120")).toEqual({
      amount_minor: 12000,
      currency: "GBP",
    });
    expect(parsePrice("120 £")).toEqual({
      amount_minor: 12000,
      currency: "GBP",
    });
  });

  it("parses other common symbols", () => {
    expect(parsePrice("$120")).toEqual({
      amount_minor: 12000,
      currency: "USD",
    });
    expect(parsePrice("€120")).toEqual({
      amount_minor: 12000,
      currency: "EUR",
    });
    expect(parsePrice("¥120")).toEqual({
      amount_minor: 120,
      currency: "JPY",
    });
  });

  it("strips thousands separators (comma)", () => {
    expect(parsePrice("£1,200")).toEqual({
      amount_minor: 120000,
      currency: "GBP",
    });
    expect(parsePrice("$1,234,567")).toEqual({
      amount_minor: 123456700,
      currency: "USD",
    });
  });

  it("parses decimals as currency's minor units", () => {
    expect(parsePrice("£120.50")).toEqual({
      amount_minor: 12050,
      currency: "GBP",
    });
    expect(parsePrice("$0.99")).toEqual({
      amount_minor: 99,
      currency: "USD",
    });
  });

  it("rejects decimals on zero-decimal currencies", () => {
    expect(() => parsePrice("JPY 12.50")).toThrow();
  });

  it("zero-pads short decimal", () => {
    expect(parsePrice("$1.5")).toEqual({
      amount_minor: 150,
      currency: "USD",
    });
  });

  it("respects an explicit 3-letter code over fallback", () => {
    expect(parsePrice("EUR 4500")).toEqual({
      amount_minor: 450000,
      currency: "EUR",
    });
    expect(parsePrice("gbp 50", "USD")).toEqual({
      amount_minor: 5000,
      currency: "GBP",
    });
  });

  it("explicit code wins over symbol", () => {
    // If you type both, the literal code is what you meant.
    expect(parsePrice("EUR $100")).toEqual({
      amount_minor: 10000,
      currency: "EUR",
    });
  });

  it("rejects clearly malformed input", () => {
    expect(() => parsePrice("not a price")).toThrow();
    expect(() => parsePrice("£")).toThrow();
    expect(() => parsePrice("$.")).toThrow();
  });

  it("rejects amounts past the overflow cap", () => {
    expect(() => parsePrice("$10,000,000,000")).toThrow();
  });
});

describe("formatPriceForInput", () => {
  it("formats USD with 2 decimals", () => {
    expect(formatPriceForInput(12000, "USD")).toBe("120.00");
    expect(formatPriceForInput(99, "USD")).toBe("0.99");
    expect(formatPriceForInput(0, "USD")).toBe("0.00");
  });

  it("formats JPY with no decimals", () => {
    expect(formatPriceForInput(12000, "JPY")).toBe("12000");
  });

  it("is the inverse of parsePrice for common currencies", () => {
    for (const [input, code] of [
      ["120.50", "GBP"],
      ["0.01", "USD"],
      ["1500", "JPY"],
    ] as const) {
      const parsed = parsePrice(input, code);
      expect(parsed).not.toBeNull();
      expect(formatPriceForInput(parsed!.amount_minor, code)).toBe(input);
    }
  });
});

describe("minorUnitsFor", () => {
  it("returns the right count for known currencies", () => {
    expect(minorUnitsFor("USD")).toBe(2);
    expect(minorUnitsFor("JPY")).toBe(0);
  });

  it("falls back to 2 for unknown codes", () => {
    expect(minorUnitsFor("XYZ")).toBe(2);
  });
});

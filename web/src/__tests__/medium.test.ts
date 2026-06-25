/**
 * `lib/medium.ts` — taxonomy constants + helpers (T-073).
 */

import { describe, expect, it } from "vitest";
import {
  MEDIUM_CATEGORIES,
  formatMedium,
  isMediumCategory,
  mediumLabel,
} from "@/lib/medium";

describe("MEDIUM_CATEGORIES", () => {
  it("matches the server-side count (11 categories)", () => {
    // Forcing function — if the server adds a category but we forget
    // to extend MEDIUM_CATEGORIES, the studio dropdown silently
    // omits it. The count is the cheapest cross-check.
    expect(MEDIUM_CATEGORIES.length).toBe(11);
  });

  it("uses snake_case codes", () => {
    for (const c of MEDIUM_CATEGORIES) {
      expect(c).toMatch(/^[a-z][a-z_]*[a-z]$/);
    }
  });
});

describe("isMediumCategory", () => {
  it("accepts canonical codes", () => {
    expect(isMediumCategory("painting")).toBe(true);
    expect(isMediumCategory("mixed_media")).toBe(true);
  });

  it("rejects unknown strings", () => {
    expect(isMediumCategory("Painting")).toBe(false);
    expect(isMediumCategory("nft")).toBe(false);
    expect(isMediumCategory("")).toBe(false);
  });
});

describe("mediumLabel", () => {
  it("title-cases known codes", () => {
    expect(mediumLabel("painting")).toBe("Painting");
    expect(mediumLabel("mixed_media")).toBe("Mixed media");
  });

  it("falls back to the raw code for unknown values", () => {
    expect(mediumLabel("nft")).toBe("nft");
  });

  it("returns empty string for null / undefined / empty", () => {
    expect(mediumLabel(null)).toBe("");
    expect(mediumLabel(undefined)).toBe("");
    expect(mediumLabel("")).toBe("");
  });
});

describe("formatMedium", () => {
  it("combines category + materials with a separator", () => {
    expect(formatMedium("painting", "Oil on linen")).toBe(
      "Painting · Oil on linen",
    );
  });

  it("shows category alone when materials missing", () => {
    expect(formatMedium("painting", null)).toBe("Painting");
    expect(formatMedium("painting", "")).toBe("Painting");
    expect(formatMedium("painting", "   ")).toBe("Painting");
  });

  it("shows materials alone when category missing (legacy rows)", () => {
    expect(formatMedium(null, "Oil on linen")).toBe("Oil on linen");
    expect(formatMedium(undefined, "Bronze")).toBe("Bronze");
  });

  it("returns empty string when both missing", () => {
    expect(formatMedium(null, null)).toBe("");
    expect(formatMedium(undefined, undefined)).toBe("");
    expect(formatMedium("", "")).toBe("");
  });
});

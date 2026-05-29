import { describe, it, expect } from "vitest";
import { normalizeWebsiteUrl } from "@/lib/normalizeUrl";

describe("normalizeWebsiteUrl", () => {
  it("returns null for empty / whitespace-only input", () => {
    expect(normalizeWebsiteUrl("")).toBeNull();
    expect(normalizeWebsiteUrl("   ")).toBeNull();
    expect(normalizeWebsiteUrl("\t\n")).toBeNull();
  });

  it("trims and preserves an existing https:// URL", () => {
    expect(normalizeWebsiteUrl("https://foo.example")).toBe(
      "https://foo.example"
    );
    expect(normalizeWebsiteUrl("  https://foo.example  ")).toBe(
      "https://foo.example"
    );
  });

  it("preserves http:// (does not silently upgrade)", () => {
    // Legacy http-only sites exist; let the server reject if it
    // wants to. We don't second-guess the user here.
    expect(normalizeWebsiteUrl("http://foo.example")).toBe(
      "http://foo.example"
    );
  });

  it("prepends https:// when no scheme is present", () => {
    expect(normalizeWebsiteUrl("guitardojo.app")).toBe(
      "https://guitardojo.app"
    );
    expect(normalizeWebsiteUrl("www.guitardojo.app")).toBe(
      "https://www.guitardojo.app"
    );
  });

  it("treats scheme matching as case-insensitive", () => {
    expect(normalizeWebsiteUrl("HTTPS://foo.example")).toBe(
      "HTTPS://foo.example"
    );
    expect(normalizeWebsiteUrl("Http://foo.example")).toBe(
      "Http://foo.example"
    );
  });

  it("preserves paths and query strings on already-schemed input", () => {
    expect(normalizeWebsiteUrl("https://foo.example/path?x=1")).toBe(
      "https://foo.example/path?x=1"
    );
  });

  it("prepends scheme even when input has a path", () => {
    expect(normalizeWebsiteUrl("foo.example/portfolio")).toBe(
      "https://foo.example/portfolio"
    );
  });
});

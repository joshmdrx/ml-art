/**
 * T-086 — `validateDraftForm`: the admin "New draft" form refuses to
 * submit without a consent note, and mirrors the API's URL + length
 * rules.
 */

import { describe, expect, it } from "vitest";
import { validateDraftForm } from "@/lib/draftValidation";

const ok = {
  display_name: "Cell Project Space",
  website_url: "https://www.cellprojects.org",
  consent_note: "Replied yes to the founding-studio email",
};

describe("validateDraftForm", () => {
  it("accepts a complete form", () => {
    expect(validateDraftForm(ok)).toEqual({});
  });

  it("allows an empty website", () => {
    expect(validateDraftForm({ ...ok, website_url: "  " })).toEqual({});
  });

  it("requires a consent note", () => {
    expect(validateDraftForm({ ...ok, consent_note: "   " }).consent_note).toBeTruthy();
  });

  it("requires a name", () => {
    expect(validateDraftForm({ ...ok, display_name: "" }).display_name).toBeTruthy();
  });

  it("rejects non-http websites", () => {
    expect(validateDraftForm({ ...ok, website_url: "cellprojects.org" }).website_url).toBeTruthy();
  });

  it("caps lengths like the API", () => {
    const errs = validateDraftForm({
      ...ok,
      display_name: "x".repeat(101),
      consent_note: "y".repeat(501),
    });
    expect(errs.display_name).toBeTruthy();
    expect(errs.consent_note).toBeTruthy();
  });
});

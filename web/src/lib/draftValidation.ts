/**
 * T-086 — client-side validation for the admin "New draft" form
 * (`components/admin/DraftCreateForm`). Mirrors the API's limits in
 * `admin::drafts::create` so most mistakes surface inline before a
 * round trip. Pure; unit-tested in `__tests__/draftValidation.test.ts`.
 */

export type DraftFormErrors = Partial<
  Record<"display_name" | "website_url" | "consent_note", string>
>;

export function validateDraftForm(v: {
  display_name: string;
  website_url: string;
  consent_note: string;
}): DraftFormErrors {
  const errors: DraftFormErrors = {};
  const name = v.display_name.trim();
  if (!name) errors.display_name = "Name is required.";
  else if (name.length > 100) errors.display_name = "Keep the name under 100 characters.";

  const url = v.website_url.trim();
  if (url && !/^https?:\/\//.test(url))
    errors.website_url = "Website must start with http:// or https://";

  const note = v.consent_note.trim();
  if (!note)
    errors.consent_note =
      "Record how they agreed, e.g. “Replied yes to the founding-studio email, 20 Nov”.";
  else if (note.length > 500) errors.consent_note = "Keep the note under 500 characters.";

  return errors;
}

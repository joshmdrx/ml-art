"use client";

/**
 * T-086 — "New draft" form on `/admin/drafts`.
 *
 * Only for organisations that have already said yes to an example
 * page: the consent note is mandatory (the API refuses without one) and
 * the copy says so. Inline form, not a modal — it lives on the page.
 * Validation is JS + `<FieldError>`; success toasts and clears the form
 * (see docs/ui-patterns.md).
 */

import { useState, useTransition } from "react";
import { useRouter } from "next/navigation";
import { toast } from "sonner";
import { FieldError } from "@/components/ui/FieldError";
import { createDraftAction } from "@/app/actions/admin";
import { toUserMessage } from "@/lib/reportError";
import { validateDraftForm, type DraftFormErrors } from "@/lib/draftValidation";

const INPUT =
  "w-full bg-background border border-border px-3 py-2 text-sm focus:outline-none focus:border-foreground";

type Errors = DraftFormErrors;

export function DraftCreateForm() {
  const router = useRouter();
  const [isPending, startTransition] = useTransition();
  const [displayName, setDisplayName] = useState("");
  const [entityType, setEntityType] = useState<"gallery" | "individual">("gallery");
  const [location, setLocation] = useState("");
  const [websiteUrl, setWebsiteUrl] = useState("");
  const [consentNote, setConsentNote] = useState("");
  const [errors, setErrors] = useState<Errors>({});
  const [formError, setFormError] = useState<string | null>(null);

  function onSubmit(e: React.FormEvent) {
    e.preventDefault();
    const next = validateDraftForm({
      display_name: displayName,
      website_url: websiteUrl,
      consent_note: consentNote,
    });
    setErrors(next);
    setFormError(null);
    if (Object.keys(next).length > 0) return;

    startTransition(async () => {
      try {
        const draft = await createDraftAction({
          display_name: displayName.trim(),
          entity_type: entityType,
          location: location.trim() || undefined,
          website_url: websiteUrl.trim() || undefined,
          consent_note: consentNote.trim(),
        });
        toast.success(`Draft created for ${draft.display_name}`);
        setDisplayName("");
        setLocation("");
        setWebsiteUrl("");
        setConsentNote("");
        router.refresh();
      } catch (err) {
        setFormError(
          toUserMessage(err, "Couldn't create the draft. Try again.", {
            surface: "admin-drafts-create",
          }),
        );
      }
    });
  }

  return (
    <form onSubmit={onSubmit} className="space-y-4 border border-border bg-surface p-5">
      <div>
        <h2 className="font-serif text-xl">New draft</h2>
        <p className="mt-1 text-sm text-muted">
          Only for organisations that have said yes to an example page.
          Build it from their public site; it stays private until they
          claim and publish it.
        </p>
      </div>

      {formError && (
        <p role="alert" className="p-3 border border-border bg-background text-sm">
          {formError}
        </p>
      )}

      <div className="grid sm:grid-cols-2 gap-4">
        <label className="block text-sm">
          <span className="block mb-1">Name</span>
          <input
            className={INPUT}
            value={displayName}
            onChange={(e) => setDisplayName(e.target.value)}
            placeholder="Cell Project Space"
          />
          <FieldError message={errors.display_name} />
        </label>
        <label className="block text-sm">
          <span className="block mb-1">Type</span>
          <select
            className={INPUT}
            value={entityType}
            onChange={(e) => setEntityType(e.target.value as "gallery" | "individual")}
          >
            <option value="gallery">Gallery / studio</option>
            <option value="individual">Individual artist</option>
          </select>
        </label>
        <label className="block text-sm">
          <span className="block mb-1">Location (optional)</span>
          <input
            className={INPUT}
            value={location}
            onChange={(e) => setLocation(e.target.value)}
            placeholder="Bethnal Green, London"
          />
        </label>
        <label className="block text-sm">
          <span className="block mb-1">Website (optional)</span>
          <input
            className={INPUT}
            value={websiteUrl}
            onChange={(e) => setWebsiteUrl(e.target.value)}
            placeholder="https://"
          />
          <FieldError message={errors.website_url} />
        </label>
      </div>

      <label className="block text-sm">
        <span className="block mb-1">How did they agree?</span>
        <textarea
          className={INPUT}
          rows={2}
          value={consentNote}
          onChange={(e) => setConsentNote(e.target.value)}
          placeholder="Replied yes to the founding-studio email, 20 Nov 2026"
        />
        <FieldError message={errors.consent_note} />
      </label>

      <button
        type="submit"
        disabled={isPending}
        className="px-4 py-2 text-sm border border-foreground bg-foreground text-background disabled:opacity-40"
      >
        {isPending ? "Creating…" : "Create draft"}
      </button>
    </form>
  );
}

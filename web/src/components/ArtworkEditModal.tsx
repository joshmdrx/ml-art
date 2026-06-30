"use client";

/**
 * Artwork create + edit modal.
 *
 * One component for both flows because they share 95% of the form. The
 * `target` prop is either:
 *   - `"new"` — render an empty form. On save, `createArtwork` runs;
 *     the result's id becomes the new target so the user can add
 *     images without a separate page transition.
 *   - `<uuid>` — render the detail form pre-filled via `getStudioArtwork`
 *
 * Status transitions (draft ↔ published ↔ archived), delete, and image
 * management all happen inside this modal too — keeps the studio
 * navigation flat.
 */

import * as Dialog from "@radix-ui/react-dialog";
import {
  useEffect,
  useRef,
  useState,
  useTransition,
  type FormEvent,
} from "react";
import { toast } from "sonner";
import {
  createArtwork,
  deleteArtwork,
  loadArtworkForEdit,
  patchArtwork,
  removeArtworkImage,
  uploadArtworkImage,
} from "@/app/actions/studio";
import type { MediumCategory, StudioArtworkDetail, StudioImage } from "@/lib/api";
import { isMediumCategory, MEDIUM_CATEGORIES, mediumLabel } from "@/lib/medium";
import { normalizeWebsiteUrl } from "@/lib/normalizeUrl";
import { formatPriceForInput, parsePrice } from "@/lib/parsePrice";
import { reportError, toUserMessage } from "@/lib/reportError";
import { FieldError } from "@/components/ui/FieldError";
import { useConfirm } from "@/components/ui/ConfirmDialog";

const AVAILABILITY_OPTIONS = [
  { value: "available", label: "Available" },
  { value: "inquire", label: "Inquire" },
  { value: "sold", label: "Sold" },
  { value: "not_for_sale", label: "Not for sale" },
] as const;

const STATUS_OPTIONS = [
  { value: "draft", label: "Draft" },
  { value: "published", label: "Published" },
  { value: "archived", label: "Archived" },
] as const;

/** Currencies offered in the price-input dropdown. Covers the
 * countries we'd expect to see real artists from at v1; the parser
 * accepts any 3-letter ISO code typed inline ("AUD 200") so this
 * list isn't a hard limit. */
const COMMON_CURRENCIES = [
  "USD",
  "GBP",
  "EUR",
  "CAD",
  "AUD",
  "JPY",
  "CHF",
  "SEK",
  "NOK",
  "DKK",
] as const;

type Target = string | "new" | null;

interface Props {
  artistDisplayName: string;
  open: boolean;
  target: Target;
  /** Fired after every successful write (create / edit / image
   *  add / remove). `closeAfter=true` only on edit-mode Save; the
   *  parent uses it (plus the current URL `?id=`) to decide URL
   *  transitions — see `StudioPortfolio.onSaved`. Optional so existing
   *  callers compile; lifecycle without it is still functional but
   *  loses the create→edit URL advance. */
  onSaved?: (detail: StudioArtworkDetail, closeAfter?: boolean) => void;
  onClose: () => void;
}

type LoadState =
  | { kind: "idle" }
  | { kind: "loading" }
  | { kind: "ready"; detail: StudioArtworkDetail | null /* null = create */ }
  | { kind: "error"; message: string };

export function ArtworkEditModal({
  artistDisplayName,
  open,
  target,
  onSaved,
  onClose,
}: Props) {
  const [load, setLoad] = useState<LoadState>({ kind: "idle" });

  // Effect-on-open: load the detail when `target` is a uuid, or pop
  // the create form when it's "new". Cleanup on close resets state so
  // reopening doesn't show stale data.
  //
  // Short-circuit: when the URL just advanced from `?id=new` to
  // `?id=<new-uuid>` after a successful create, our local `load`
  // already holds the just-created detail. Skip the refetch (would
  // cause a "Loading…" flash) — the create path called the inner
  // `onSaved` which lifted state in place.
  useEffect(() => {
    if (!open) return;
    if (target === null) return;
    if (target === "new") {
      // Intentional state-machine transition on `open` — same pattern
      // (and the same conservative lint exception) as SaveModal /
      // InquiryModal.
      // eslint-disable-next-line react-hooks/set-state-in-effect
      setLoad({ kind: "ready", detail: null });
      return;
    }
    // T-058-style URL-driven lifecycle: short-circuit if local state
    // already has this detail (avoids the refetch flash on the
    // create-mode → edit-mode URL transition).
    if (load.kind === "ready" && load.detail?.id === target) return;
    let cancelled = false;
    setLoad({ kind: "loading" });
    loadArtworkForEdit(target)
      .then((detail) => {
        if (cancelled) return;
        if (!detail) {
          setLoad({ kind: "error", message: "Couldn't load this artwork." });
        } else {
          setLoad({ kind: "ready", detail });
        }
      })
      .catch((e) => {
        if (!cancelled) {
          setLoad({
            kind: "error",
            message: toUserMessage(e, "Couldn't load this artwork.", {
              surface: "artwork-edit-modal",
              target,
            }),
          });
        }
      });
    return () => {
      cancelled = true;
    };
    // `load` is intentionally NOT a dep — this effect's job is to
    // SET load, so re-running when it changes would loop. The
    // short-circuit read above is a one-shot guard against the
    // create-mode → edit-mode URL flip refetching what we just got.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [open, target]);

  function handleOpenChange(next: boolean) {
    if (!next) {
      setLoad({ kind: "idle" });
      onClose();
    }
  }

  return (
    <Dialog.Root open={open} onOpenChange={handleOpenChange}>
      <Dialog.Portal>
        <Dialog.Overlay className="fixed inset-0 bg-foreground/30 backdrop-blur-sm z-40" />
        <Dialog.Content
          className="fixed left-1/2 top-1/2 -translate-x-1/2 -translate-y-1/2 z-50 w-[95vw] max-w-2xl max-h-[90vh] overflow-y-auto bg-surface border border-border p-6 shadow-2xl"
          aria-describedby={undefined}
        >
          <Dialog.Title className="font-serif text-2xl mb-1">
            {target === "new" ? "New artwork" : "Edit artwork"}
          </Dialog.Title>
          <Dialog.Description className="sr-only">
            Fields for the artwork: title, medium, price, status, and image
            management.
          </Dialog.Description>

          {load.kind === "loading" && (
            <p className="text-sm text-muted py-6">Loading…</p>
          )}
          {load.kind === "error" && (
            <p className="text-sm py-4">{load.message}</p>
          )}
          {load.kind === "ready" && (
            <ArtworkForm
              detail={load.detail}
              artistDisplayName={artistDisplayName}
              onSaved={(detail, closeAfter) => {
                // Two paths, mutually exclusive:
                //  - closeAfter=true (edit Save): close the modal.
                //    `onClose` drives the URL clear in the parent.
                //  - closeAfter=false (create / image add/remove):
                //    lift state in place so the artist keeps editing,
                //    and bubble `onSaved` so the parent can advance
                //    `?id=new` → `?id=<new-uuid>` (no-op for images).
                if (closeAfter) {
                  onClose();
                } else {
                  setLoad({ kind: "ready", detail });
                  onSaved?.(detail, false);
                }
              }}
              onDeleted={onClose}
            />
          )}

          <Dialog.Close asChild>
            <button
              type="button"
              aria-label="Close"
              className="absolute top-3 right-3 text-muted hover:text-foreground"
            >
              ×
            </button>
          </Dialog.Close>
        </Dialog.Content>
      </Dialog.Portal>
    </Dialog.Root>
  );
}

// ─────────────────────────────────────────────────────────────────────────────
// Form body
// ─────────────────────────────────────────────────────────────────────────────

function ArtworkForm({
  detail,
  artistDisplayName,
  onSaved,
  onDeleted,
}: {
  detail: StudioArtworkDetail | null;
  artistDisplayName: string;
  /** Called after a successful save (or in-modal image change).
   * `closeAfter=true` tells the parent to close; falsy lifts state in
   * place. Default-false because the riskier behaviour (closing) must
   * be opt-in. */
  onSaved: (detail: StudioArtworkDetail, closeAfter?: boolean) => void;
  onDeleted: () => void;
}) {
  const isCreate = detail === null;
  const confirm = useConfirm();

  const [title, setTitle] = useState(detail?.title ?? "");
  const [description, setDescription] = useState(detail?.description ?? "");
  // T-073 — `medium` is now the free-text "materials" field; the
  // canonical filter lives on `medium_category`.
  const [medium, setMedium] = useState(detail?.medium ?? "");
  const [mediumCategory, setMediumCategory] = useState<string>(
    detail?.medium_category ?? "",
  );
  const [yearCreated, setYearCreated] = useState(
    detail?.year_created != null ? String(detail.year_created) : ""
  );
  const [yearError, setYearError] = useState<string | null>(null);

  /** Validate the year input. Empty is fine (year is optional); a
   * value must be a whole integer 1000–2100 — matches the historical
   * `min`/`max` constraints we used to set on the <input>, now lifted
   * into JS so every validation message lands as a `<FieldError>`.
   * Returns null on valid input, the message string on invalid. */
  function validateYear(): string | null {
    const raw = yearCreated.trim();
    if (raw === "") return null;
    const n = Number(raw);
    if (!Number.isFinite(n) || !Number.isInteger(n)) {
      return "Year must be a whole number.";
    }
    if (n < 1000 || n > 2100) {
      return "Year must be between 1000 and 2100.";
    }
    return null;
  }
  // Price is a free-text input (T-039) — the artist types "£120" or
  // "120.50", we parse to minor units on submit. State holds the raw
  // display string, not the integer.
  const [priceInput, setPriceInput] = useState(
    detail?.price_cents != null && detail?.currency
      ? formatPriceForInput(detail.price_cents, detail.currency)
      : ""
  );
  // Default for new artworks is GBP (T-080 — UK focus). Existing
  // artworks keep whatever currency they were saved with.
  const [currency, setCurrency] = useState(detail?.currency ?? "GBP");
  const [priceError, setPriceError] = useState<string | null>(null);

  /** Parse the current price input into `{ amount_minor, currency }`
   * or null (empty input). Sets `priceError` and returns `undefined`
   * if the input is malformed — caller bails out of the submit. */
  function tryParsePrice(): { amount_minor: number; currency: string } | null | undefined {
    try {
      const result = parsePrice(priceInput, currency);
      setPriceError(null);
      return result;
    } catch (e) {
      setPriceError(e instanceof Error ? e.message : "Couldn't parse price");
      return undefined;
    }
  }

  /** On blur: re-format to the canonical "120.00" shape so artists
   * see what's being stored. No-op on empty or unparseable input. */
  function onPriceBlur() {
    if (priceInput.trim().length === 0) return;
    try {
      const parsed = parsePrice(priceInput, currency);
      if (parsed) {
        setPriceInput(formatPriceForInput(parsed.amount_minor, parsed.currency));
        setCurrency(parsed.currency);
        setPriceError(null);
      }
    } catch {
      // Leave the input as-is; the submit-time validator will
      // surface the error.
    }
  }
  const [availability, setAvailability] = useState<string>(
    detail?.availability ?? "available"
  );
  const [externalUrl, setExternalUrl] = useState(detail?.external_url ?? "");
  const [status, setStatus] = useState<string>(detail?.status ?? "draft");

  // T-070 — physical artwork dimensions in cm. All three optional;
  // see buildDimensions() for the all-or-nothing-on-width+height rule.
  const [widthCm, setWidthCm] = useState(
    detail?.dimensions?.width != null ? String(detail.dimensions.width) : "",
  );
  const [heightCm, setHeightCm] = useState(
    detail?.dimensions?.height != null ? String(detail.dimensions.height) : "",
  );
  const [depthCm, setDepthCm] = useState(
    detail?.dimensions?.depth != null ? String(detail.dimensions.depth) : "",
  );
  const [dimsError, setDimsError] = useState<string | null>(null);

  const [error, setError] = useState<string | null>(null);
  const [isPending, startTransition] = useTransition();

  /**
   * Build the validated dimensions value, mirroring the server-side
   * `core::validation::dimensions_v1` checks. Three outcomes:
   *
   *   - all blank → `{ value: undefined, error: null }` — caller
   *     omits the field; server leaves the column alone.
   *   - any field invalid → `{ value: undefined, error: <msg> }` —
   *     caller surfaces inline; we don't submit.
   *   - valid → `{ value: { unit: "cm", width, height, depth? } }`.
   *
   * Width + height are all-or-nothing: setting one without the other
   * is a usability footgun (the "S/M/L" filter would silently skip
   * the row). Depth is optional.
   */
  function buildDimensions(): {
    value: import("@/lib/api").Dimensions | undefined;
    error: string | null;
  } {
    const w = widthCm.trim();
    const h = heightCm.trim();
    const d = depthCm.trim();
    if (w === "" && h === "" && d === "") return { value: undefined, error: null };
    if (w === "" || h === "") {
      return {
        value: undefined,
        error: "Width and height are both required when entering dimensions.",
      };
    }
    function parseDim(raw: string, label: string): number | string {
      const n = Number(raw);
      if (!Number.isFinite(n) || !Number.isInteger(n))
        return `${label} must be a whole number in cm.`;
      if (n < 1) return `${label} must be at least 1 cm.`;
      if (n > 5000) return `${label} must be 5000 cm or less.`;
      return n;
    }
    const wn = parseDim(w, "Width");
    if (typeof wn !== "number") return { value: undefined, error: wn };
    const hn = parseDim(h, "Height");
    if (typeof hn !== "number") return { value: undefined, error: hn };
    const dims: import("@/lib/api").Dimensions = {
      unit: "cm",
      width: wn,
      height: hn,
    };
    if (d !== "") {
      const dn = parseDim(d, "Depth");
      if (typeof dn !== "number") return { value: undefined, error: dn };
      dims.depth = dn;
    }
    return { value: dims, error: null };
  }

  function buildBody(parsedPrice: { amount_minor: number; currency: string } | null) {
    const dims = buildDimensions();
    return {
      title: title.trim() || null,
      description: description.trim() || null,
      medium: medium.trim() || null,
      // T-073 — canonical category. Same `null = clear` rule as
      // dimensions (T-072 deserialize_double_option). isMediumCategory
      // guards against the select holding an unknown value (shouldn't
      // happen — the <select> options are bounded — but defensive).
      medium_category:
        mediumCategory && isMediumCategory(mediumCategory)
          ? (mediumCategory as MediumCategory)
          : null,
      year_created: yearCreated ? Number(yearCreated) : null,
      // T-072 — `null` (not `undefined`) so the server-side
      // `deserialize_double_option` reads this as Some(None) and
      // clears the column. Sending `undefined` here would serialise
      // as omitted, which means "leave the existing value alone" —
      // not what an artist who blanked the dims expects.
      dimensions: dims.value ?? null,
      price_cents: parsedPrice?.amount_minor ?? null,
      currency: parsedPrice?.currency ?? (currency.trim() || "GBP"),
      availability: availability as
        | "available"
        | "sold"
        | "not_for_sale"
        | "inquire",
      external_url: normalizeWebsiteUrl(externalUrl),
      status: status as "draft" | "published" | "archived",
    };
  }

  async function onSubmit(e: FormEvent<HTMLFormElement>) {
    e.preventDefault();
    if (isPending) return;
    setError(null);
    setDimsError(null);
    setYearError(null);

    const parsedPrice = tryParsePrice();
    if (parsedPrice === undefined) {
      // tryParsePrice already set priceError + this is a synchronous
      // bail. Don't fire the network call.
      return;
    }

    // T-071 — year validation runs in JS now (the HTML min/max
    // attributes were removed because they showed browser-native
    // tooltips that didn't match our FieldError styling).
    const yearErr = validateYear();
    if (yearErr) {
      setYearError(yearErr);
      return;
    }

    // T-070 — validate dimensions client-side. Server also validates
    // (single source of truth) but a synchronous bail saves a
    // round-trip and gives the artist a field-anchored error.
    const dims = buildDimensions();
    if (dims.error) {
      setDimsError(dims.error);
      return;
    }

    // T-070 + T-073 — soft nudge when publishing a work that still
    // has no dimensions OR no category. Buyers can't filter by size
    // for works with NULL dimensions, and can't filter by medium for
    // works with NULL medium_category. We ask before letting the
    // artist publish without — but don't gate. Combined into one
    // dialog so a doubly-incomplete publish only prompts once.
    const isTransitionToPublished =
      status === "published" && detail?.status !== "published";
    if ((isCreate ? false : isTransitionToPublished)) {
      const missing: string[] = [];
      if (dims.value === undefined) missing.push("dimensions");
      if (!mediumCategory) missing.push("a medium category");
      if (missing.length > 0) {
        const what = missing.join(" or ");
        const proceed = await confirm({
          title: `Publish without ${what}?`,
          description:
            "Buyers won't be able to filter your work by " +
            missing.join(" or ") +
            " until you add them. You can edit and add later.",
          confirmLabel: "Publish anyway",
          cancelLabel: "Keep editing",
        });
        if (!proceed) return;
      }
    }

    startTransition(async () => {
      try {
        if (isCreate) {
          const created = await createArtwork({
            title: title.trim() || undefined,
            description: description.trim() || undefined,
            medium: medium.trim() || undefined,
            medium_category:
              mediumCategory && isMediumCategory(mediumCategory)
                ? (mediumCategory as MediumCategory)
                : undefined,
            year_created: yearCreated ? Number(yearCreated) : undefined,
            dimensions: dims.value,
            price_cents: parsedPrice?.amount_minor ?? undefined,
            currency: parsedPrice?.currency ?? (currency.trim() || undefined),
            availability: availability as
              | "available"
              | "sold"
              | "not_for_sale"
              | "inquire",
            external_url: normalizeWebsiteUrl(externalUrl) ?? undefined,
          });
          toast.success("Artwork created — add an image below.");
          // Lift to "ready with detail" so image management activates.
          // The created row has no images yet, so synthesize an empty
          // detail with the shape the modal expects. closeAfter omitted
          // → stay open so the artist can keep filling in dimensions,
          // images, etc.
          onSaved({
            ...created,
            description: description.trim() || null,
            year_created: yearCreated ? Number(yearCreated) : null,
            dimensions: dims.value ?? null,
            external_url: normalizeWebsiteUrl(externalUrl),
            images: [],
          });
        } else {
          const updated = await patchArtwork(detail!.id, buildBody(parsedPrice));
          toast.success("Saved");
          // closeAfter=true → parent closes the modal. T-071 default
          // for edit flow.
          onSaved({ ...detail!, ...updated }, true);
        }
      } catch (e) {
        setError(
          toUserMessage(e, "Couldn't save this artwork. Try again.", {
            surface: "artwork-edit-modal",
            call: "save",
          }),
        );
      }
    });
  }

  async function onDelete() {
    if (!detail || isPending) return;
    const proceed = await confirm({
      title: `Delete “${detail.title ?? "Untitled"}”?`,
      description: "This can't be undone.",
      confirmLabel: "Delete",
      destructive: true,
    });
    if (!proceed) return;
    setError(null);
    startTransition(async () => {
      try {
        await deleteArtwork(detail.id);
        toast.success("Artwork deleted");
        onDeleted();
      } catch (e) {
        setError(
          toUserMessage(e, "Couldn't delete this artwork. Try again.", {
            surface: "artwork-edit-modal",
            call: "delete",
          }),
        );
      }
    });
  }

  return (
    <form onSubmit={onSubmit} className="mt-4 space-y-5">
      {error && (
        <p role="alert" className="p-3 border border-border bg-background text-sm">
          {error}
        </p>
      )}

      <p className="text-xs text-muted">
        Listed under <strong>{artistDisplayName}</strong>.
      </p>

      <Field label="Title">
        <input
          type="text"
          value={title}
          onChange={(e) => setTitle(e.target.value)}
          maxLength={200}
          className="w-full bg-background border border-border px-3 py-2 text-sm focus:outline-none focus:border-foreground"
        />
      </Field>

      {/* T-073 — category is the canonical taxonomy bucket (filterable).
          Materials underneath is free text for the specifics. Display
          combines them as "Painting · Oil on linen". */}
      <Field
        label="Category"
        hint="Buyers filter by this. Pick the closest match — describe the specifics below."
      >
        <select
          value={mediumCategory}
          onChange={(e) => setMediumCategory(e.target.value)}
          className="w-full bg-background border border-border px-3 py-2 text-sm focus:outline-none focus:border-foreground"
        >
          <option value="">— Select a category —</option>
          {MEDIUM_CATEGORIES.map((code) => (
            <option key={code} value={code}>
              {mediumLabel(code)}
            </option>
          ))}
        </select>
      </Field>

      <Field
        label="Materials"
        hint="Optional. e.g. Oil on linen, Inkjet print, Bronze."
      >
        <input
          type="text"
          value={medium}
          onChange={(e) => setMedium(e.target.value)}
          className="w-full bg-background border border-border px-3 py-2 text-sm focus:outline-none focus:border-foreground"
        />
      </Field>

      <Field label="Description">
        <textarea
          value={description}
          onChange={(e) => setDescription(e.target.value)}
          rows={4}
          maxLength={8_000}
          className="w-full bg-background border border-border px-3 py-2 text-sm focus:outline-none focus:border-foreground"
        />
      </Field>

      <div className="grid grid-cols-2 gap-4">
        <Field label="Year created">
          <input
            type="number"
            value={yearCreated}
            onChange={(e) => {
              setYearCreated(e.target.value);
              if (yearError) setYearError(null);
            }}
            inputMode="numeric"
            className="w-full bg-background border border-border px-3 py-2 text-sm focus:outline-none focus:border-foreground"
          />
          <FieldError message={yearError} />
        </Field>
        <Field
          label="Price"
          hint="Leave blank to hide. You can type the symbol — '£120', '$1,200', '4500'."
        >
          <div className="flex gap-2">
            <input
              type="text"
              inputMode="decimal"
              value={priceInput}
              onChange={(e) => {
                setPriceInput(e.target.value);
                if (priceError) setPriceError(null);
              }}
              onBlur={onPriceBlur}
              placeholder="120.00"
              className="flex-1 bg-background border border-border px-3 py-2 text-sm focus:outline-none focus:border-foreground"
            />
            <select
              aria-label="Currency"
              value={currency}
              onChange={(e) => setCurrency(e.target.value)}
              className="bg-background border border-border px-2 py-2 text-sm focus:outline-none focus:border-foreground"
            >
              {COMMON_CURRENCIES.map((c) => (
                <option key={c} value={c}>
                  {c}
                </option>
              ))}
            </select>
          </div>
          <FieldError message={priceError} />
        </Field>
      </div>

      <Field
        label="Dimensions"
        hint="Physical size in cm. Width and height are both required if you fill any. Depth is optional — leave blank for flat work."
      >
        <div className="grid grid-cols-3 gap-2">
          <input
            type="number"
            inputMode="numeric"
            value={widthCm}
            onChange={(e) => {
              setWidthCm(e.target.value);
              if (dimsError) setDimsError(null);
            }}
            placeholder="Width"
            aria-label="Width in cm"
            className="bg-background border border-border px-3 py-2 text-sm focus:outline-none focus:border-foreground"
          />
          <input
            type="number"
            inputMode="numeric"
            value={heightCm}
            onChange={(e) => {
              setHeightCm(e.target.value);
              if (dimsError) setDimsError(null);
            }}
            placeholder="Height"
            aria-label="Height in cm"
            className="bg-background border border-border px-3 py-2 text-sm focus:outline-none focus:border-foreground"
          />
          <input
            type="number"
            inputMode="numeric"
            value={depthCm}
            onChange={(e) => {
              setDepthCm(e.target.value);
              if (dimsError) setDimsError(null);
            }}
            placeholder="Depth (optional)"
            aria-label="Depth in cm, optional"
            className="bg-background border border-border px-3 py-2 text-sm focus:outline-none focus:border-foreground"
          />
        </div>
        <FieldError message={dimsError} />
      </Field>

      <div>
        <Field label="Availability">
          <select
            value={availability}
            onChange={(e) => setAvailability(e.target.value)}
            className="w-full bg-background border border-border px-3 py-2 text-sm focus:outline-none focus:border-foreground"
          >
            {AVAILABILITY_OPTIONS.map((o) => (
              <option key={o.value} value={o.value}>
                {o.label}
              </option>
            ))}
          </select>
        </Field>
      </div>

      <Field
        label="External URL"
        hint="Where buyers should go (your site, gallery page, etc.). We'll add https:// for you."
      >
        <input
          type="text"
          inputMode="url"
          autoCapitalize="none"
          autoCorrect="off"
          spellCheck={false}
          value={externalUrl}
          onChange={(e) => setExternalUrl(e.target.value)}
          maxLength={500}
          placeholder="yoursite.com/painting-1"
          className="w-full bg-background border border-border px-3 py-2 text-sm focus:outline-none focus:border-foreground"
        />
      </Field>

      <Field label="Status">
        <select
          value={status}
          onChange={(e) => setStatus(e.target.value)}
          disabled={isCreate}
          className="w-full bg-background border border-border px-3 py-2 text-sm focus:outline-none focus:border-foreground disabled:opacity-60"
        >
          {STATUS_OPTIONS.map((o) => (
            <option key={o.value} value={o.value}>
              {o.label}
            </option>
          ))}
        </select>
        {isCreate && (
          <span className="block text-xs text-muted mt-1">
            New artworks start as drafts. Publish from this menu after saving.
          </span>
        )}
      </Field>

      <div className="flex items-center justify-between pt-4 border-t border-border">
        <div className="flex gap-2">
          <button
            type="submit"
            disabled={isPending}
            className="px-5 py-2 text-sm bg-foreground text-background disabled:opacity-40"
          >
            {isPending ? "Saving…" : isCreate ? "Create" : "Save"}
          </button>
          {!isCreate && (
            <button
              type="button"
              onClick={onDelete}
              disabled={isPending}
              className="px-3 py-2 text-sm text-muted hover:text-foreground"
            >
              Delete
            </button>
          )}
        </div>
      </div>

      {/* Image management — only enabled after the artwork exists. */}
      {!isCreate && detail && (
        <ImageManager
          artworkId={detail.id}
          images={detail.images}
          onChanged={(images) => onSaved({ ...detail, images })}
        />
      )}
    </form>
  );
}

// ─────────────────────────────────────────────────────────────────────────────
// Image management (sub-section of the modal)
// ─────────────────────────────────────────────────────────────────────────────

function ImageManager({
  artworkId,
  images,
  onChanged,
}: {
  artworkId: string;
  images: StudioImage[];
  onChanged: (images: StudioImage[]) => void;
}) {
  const fileInputRef = useRef<HTMLInputElement | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [isPending, startTransition] = useTransition();
  /** Progress for the in-flight batch: `{done, total}` while uploads
   * are running, `null` when idle. Drives the "Uploading N of M…"
   * caption. T-011 Phase 5. */
  const [batchStatus, setBatchStatus] = useState<{
    done: number;
    total: number;
  } | null>(null);

  /**
   * Bulk-friendly file-picker handler (T-011 Phase 5).
   *
   * Pre-checks size + MIME per file before the round-trip; bad files
   * are dropped with a per-file note rather than failing the whole
   * batch. Uploads sequentially (one at a time) — embedding work
   * server-side is non-trivial and we don't want to thunder the API
   * with parallel requests. For 1–10 images the user-perceived latency
   * is dominated by the network anyway, so the simpler model wins.
   *
   * Cap at MAX_BATCH_SIZE so a misclicked "select all" on a 500-photo
   * folder doesn't try to upload everything; excess is dropped with
   * a warning the user can act on.
   *
   * Each successful upload is appended to `images` immediately —
   * visible incremental progress, and the rejection-by-server case
   * (e.g. the 11th file is corrupt) doesn't lose the prior 10.
   */
  function onFilesSelected(files: FileList | null) {
    if (!files || files.length === 0) return;
    setError(null);

    const MAX_BATCH_SIZE = 20;
    const all = Array.from(files);
    const dropped: string[] = [];
    if (all.length > MAX_BATCH_SIZE) {
      dropped.push(
        `Skipped ${all.length - MAX_BATCH_SIZE} files — please upload at most ${MAX_BATCH_SIZE} at a time.`,
      );
    }
    const batch = all.slice(0, MAX_BATCH_SIZE);
    const valid: File[] = [];
    for (const f of batch) {
      if (!f.type.startsWith("image/")) {
        dropped.push(`${f.name}: not an image`);
        continue;
      }
      if (f.size > 10 * 1024 * 1024) {
        dropped.push(`${f.name}: larger than 10MB`);
        continue;
      }
      valid.push(f);
    }

    if (valid.length === 0) {
      setError(dropped.join("\n"));
      if (fileInputRef.current) fileInputRef.current.value = "";
      return;
    }

    setBatchStatus({ done: 0, total: valid.length });

    startTransition(async () => {
      const errors: string[] = [...dropped];
      let running = images;
      for (let i = 0; i < valid.length; i++) {
        const file = valid[i];
        try {
          const bytes = new Uint8Array(await file.arrayBuffer());
          const img = await uploadArtworkImage(artworkId, {
            name: file.name || "upload.bin",
            type: file.type || "application/octet-stream",
            bytes,
          });
          running = [...running, img];
          onChanged(running);
        } catch (e) {
          // Per-file failures get a generic line; details go to Sentry.
          errors.push(
            `${file.name}: ${toUserMessage(e, "couldn't upload", {
              surface: "studio-artwork-image-upload",
              file: file.name,
            })}`,
          );
        }
        setBatchStatus({ done: i + 1, total: valid.length });
      }
      // Reset the input so picking the same files twice in a row
      // still fires `change`.
      if (fileInputRef.current) fileInputRef.current.value = "";
      setBatchStatus(null);
      if (errors.length > 0) setError(errors.join("\n"));
    });
  }

  function onRemove(imageId: string) {
    if (isPending) return;
    setError(null);
    startTransition(async () => {
      try {
        await removeArtworkImage(artworkId, imageId);
        onChanged(images.filter((i) => i.id !== imageId));
      } catch (e) {
        setError(
          toUserMessage(e, "Couldn't remove that image. Try again.", {
            surface: "artwork-edit-modal",
            call: "remove-image",
          }),
        );
      }
    });
  }

  return (
    <section
      aria-labelledby="image-manager-heading"
      className="mt-6 pt-6 border-t border-border"
    >
      <h3 id="image-manager-heading" className="font-medium text-sm mb-3">
        Images
      </h3>

      {error && (
        <p
          role="alert"
          className="p-3 mb-3 border border-border bg-background text-sm whitespace-pre-line"
        >
          {error}
        </p>
      )}

      {images.length === 0 ? (
        <p className="text-xs text-muted mb-3">
          No images yet. The first image you add becomes the primary.
        </p>
      ) : (
        <ul className="grid grid-cols-3 gap-3 mb-4">
          {images.map((img) => (
            <li
              key={img.id}
              className="relative aspect-square border border-border bg-background"
            >
              {/* eslint-disable-next-line @next/next/no-img-element */}
              <img
                src={img.url}
                alt=""
                className={
                  // Rejected images stay in the studio (the artist
                  // needs to be able to inspect + delete them) but
                  // dim + grey so it's visually obvious they're not
                  // appearing publicly.
                  img.moderation_status === "rejected"
                    ? "absolute inset-0 w-full h-full object-cover opacity-40 grayscale"
                    : "absolute inset-0 w-full h-full object-cover"
                }
              />
              {img.is_primary && (
                <span className="absolute top-1 left-1 bg-foreground text-background text-[10px] px-1.5 py-0.5">
                  Primary
                </span>
              )}
              <ModerationBadge img={img} />
              <button
                type="button"
                onClick={() => onRemove(img.id)}
                className="absolute top-1 right-1 bg-surface text-foreground text-[10px] px-1.5 py-0.5 border border-border"
              >
                Remove
              </button>
            </li>
          ))}
        </ul>
      )}

      <div>
        <input
          ref={fileInputRef}
          type="file"
          accept="image/*"
          multiple
          onChange={(e) => onFilesSelected(e.target.files)}
          disabled={isPending}
          className="block text-sm file:mr-3 file:px-4 file:py-2 file:border file:border-border file:bg-background file:text-sm file:cursor-pointer hover:file:bg-surface disabled:opacity-40"
        />
        {isPending && batchStatus && (
          <p className="mt-2 text-xs text-muted" role="status">
            Uploading {batchStatus.done} of {batchStatus.total}
            {batchStatus.done < batchStatus.total ? "…" : " — finishing up"}
          </p>
        )}
      </div>
      <p className="text-[11px] text-muted mt-2">
        JPEG, PNG, or WebP — up to 10MB each, up to 20 at a time. The
        first image you add becomes the primary and is what shows up
        in search results.
      </p>
    </section>
  );
}

// ─────────────────────────────────────────────────────────────────────────────
// Small primitives
// ─────────────────────────────────────────────────────────────────────────────

function Field({
  label,
  hint,
  children,
}: {
  label: string;
  hint?: string;
  children: React.ReactNode;
}) {
  return (
    <label className="block">
      <span className="block font-medium text-sm mb-1">{label}</span>
      {hint && <span className="block text-xs text-muted mb-2">{hint}</span>}
      {children}
    </label>
  );
}

/**
 * Visual + textual hint of an image's moderation state on the studio
 * thumbnail tile (T-008c).
 *
 * - **pending**: amber "Checking…" — the worker hasn't graded the
 *   image yet. Common right after upload; usually flips within a
 *   few seconds.
 * - **rejected**: red "Hidden · <labels>" — image is suppressed
 *   from public surfaces. We show the comma-joined Rekognition
 *   labels (`moderation_reason`) so the artist knows what triggered
 *   it and whether to delete/replace. Note: this is the artist's
 *   diagnostic surface; we do NOT show these labels to the public.
 * - **approved**: no badge (cleaner thumbnail; the absence of a
 *   warning *is* the signal).
 */
function ModerationBadge({ img }: { img: StudioImage }) {
  if (img.moderation_status === "approved") return null;
  if (img.moderation_status === "pending") {
    return (
      <span
        className="absolute bottom-1 left-1 right-1 bg-amber-100 text-amber-900 text-[10px] px-1.5 py-0.5 truncate"
        title="The moderation worker hasn't graded this image yet."
      >
        Checking…
      </span>
    );
  }
  const labels = img.moderation_reason?.trim();
  return (
    <span
      className="absolute bottom-1 left-1 right-1 bg-red-100 text-red-900 text-[10px] px-1.5 py-0.5 truncate"
      title={
        labels
          ? `Hidden from public surfaces. Labels: ${labels}.`
          : "Hidden from public surfaces."
      }
    >
      {labels ? `Hidden · ${labels}` : "Hidden"}
    </span>
  );
}

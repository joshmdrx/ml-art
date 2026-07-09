"use client";

/**
 * Shipping form for the buy flow (M-05). Collects the destination
 * address, then opens Stripe Checkout via the `startCheckout` server
 * action and redirects the browser to the hosted-checkout URL.
 *
 * Validation is JS-only (per docs/ui-patterns.md) — errors render through
 * `<FieldError>`, never HTML `required`/`pattern`.
 */

import { useState, useTransition, type FormEvent } from "react";
import { FieldError } from "@/components/ui/FieldError";
import { toUserMessage } from "@/lib/reportError";
import { startCheckout } from "@/app/actions/orders";
import type { ShippingAddress } from "@/lib/api";

type Errors = Partial<Record<keyof ShippingAddress, string>> & {
  form?: string;
};

export function BuyForm({
  artworkId,
  defaultName,
}: {
  artworkId: string;
  defaultName: string;
}) {
  const [name, setName] = useState(defaultName);
  const [line1, setLine1] = useState("");
  const [line2, setLine2] = useState("");
  const [city, setCity] = useState("");
  const [postalCode, setPostalCode] = useState("");
  const [country, setCountry] = useState("GB");
  const [errors, setErrors] = useState<Errors>({});
  const [isPending, startTransition] = useTransition();

  function validate(): Errors {
    const e: Errors = {};
    if (!name.trim()) e.name = "Recipient name is required.";
    if (!line1.trim()) e.line1 = "Address line 1 is required.";
    if (!city.trim()) e.city = "City is required.";
    if (!postalCode.trim()) e.postal_code = "Postcode is required.";
    if (country.trim().length !== 2)
      e.country = "Use a 2-letter country code (e.g. GB).";
    return e;
  }

  function onSubmit(ev: FormEvent<HTMLFormElement>) {
    ev.preventDefault();
    if (isPending) return;

    const found = validate();
    setErrors(found);
    if (Object.keys(found).length > 0) return;

    const shipping: ShippingAddress = {
      name: name.trim(),
      line1: line1.trim(),
      line2: line2.trim() || undefined,
      city: city.trim(),
      postal_code: postalCode.trim(),
      country: country.trim().toUpperCase(),
    };

    startTransition(async () => {
      try {
        const { checkout_url } = await startCheckout(artworkId, shipping);
        // Hand off to Stripe's hosted checkout.
        window.location.href = checkout_url;
      } catch (err) {
        setErrors({
          form: toUserMessage(
            err,
            "Couldn't start checkout. Please try again.",
            { surface: "buy-form", artworkId }
          ),
        });
      }
    });
  }

  return (
    <form onSubmit={onSubmit} className="mt-8 space-y-4">
      <h2 className="font-serif text-xl">Shipping address</h2>

      <Field label="Recipient name" value={name} onChange={setName} error={errors.name} />
      <Field label="Address line 1" value={line1} onChange={setLine1} error={errors.line1} />
      <Field label="Address line 2 (optional)" value={line2} onChange={setLine2} />
      <div className="grid grid-cols-2 gap-4">
        <Field label="City" value={city} onChange={setCity} error={errors.city} />
        <Field label="Postcode" value={postalCode} onChange={setPostalCode} error={errors.postal_code} />
      </div>
      <Field
        label="Country"
        value={country}
        onChange={setCountry}
        error={errors.country}
        helper="2-letter ISO code, e.g. GB"
        maxLength={2}
      />

      {errors.form && (
        <p className="text-sm text-foreground bg-background border border-border p-3">
          {errors.form}
        </p>
      )}

      <button
        type="submit"
        disabled={isPending}
        className="w-full py-3 px-4 bg-foreground text-background text-sm hover:bg-foreground/90 transition-colors disabled:opacity-40"
      >
        {isPending ? "Redirecting to secure checkout…" : "Continue to payment"}
      </button>
    </form>
  );
}

function Field({
  label,
  value,
  onChange,
  error,
  helper,
  maxLength,
}: {
  label: string;
  value: string;
  onChange: (v: string) => void;
  error?: string;
  helper?: string;
  maxLength?: number;
}) {
  return (
    <label className="block">
      <span className="block text-xs text-muted mb-1">{label}</span>
      <input
        type="text"
        value={value}
        onChange={(e) => onChange(e.target.value)}
        maxLength={maxLength}
        className="w-full bg-background border border-border px-3 py-2 text-sm focus:outline-none focus:border-foreground"
      />
      {helper && <span className="block text-xs text-muted mt-1">{helper}</span>}
      <FieldError message={error} />
    </label>
  );
}

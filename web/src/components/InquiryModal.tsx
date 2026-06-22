"use client";

/**
 * Inquiry modal — opens from the "Inquire" button on artwork detail.
 *
 * Two branches:
 *   - Signed-in: name/email pre-filled from Clerk; email is readonly and
 *     the API ignores any body value, using the verified address instead.
 *   - Signed-out: full form. After submit, we render a "check your inbox"
 *     state with a hint that the link is single-use.
 */

import * as Dialog from "@radix-ui/react-dialog";
import { useUser } from "@clerk/nextjs";
import { useEffect, useState, useTransition, type FormEvent } from "react";
import { toUserMessage } from "@/lib/reportError";
import { sendInquiry } from "@/app/actions/inquiries";
import type { InquiryAck } from "@/lib/api";

type SubmitState =
  | { kind: "idle" }
  | { kind: "submitting" }
  | { kind: "success"; ack: InquiryAck }
  | { kind: "error"; message: string };

export function InquiryModal({
  open,
  onOpenChange,
  artworkId,
  artistName,
}: {
  open: boolean;
  onOpenChange: (next: boolean) => void;
  artworkId: string;
  artistName: string;
}) {
  const { user, isSignedIn, isLoaded } = useUser();
  const [name, setName] = useState("");
  const [email, setEmail] = useState("");
  const [message, setMessage] = useState("");
  const [budget, setBudget] = useState("");
  const [state, setState] = useState<SubmitState>({ kind: "idle" });
  const [isPending, startTransition] = useTransition();

  // Reset on close is driven by the close action, not a sync effect.
  // See `SaveModal.tsx` for the same pattern + rationale.
  function handleOpenChange(next: boolean) {
    if (!next) {
      setState({ kind: "idle" });
      setMessage("");
      setBudget("");
    }
    onOpenChange(next);
  }

  // Pre-fill from Clerk when the modal opens. The setStates inside the
  // effect are intentional: we're syncing local form state from an
  // external system (Clerk's user object) that resolves asynchronously
  // after first render. See `SaveModal.tsx` for the same eslint dance.
  useEffect(() => {
    if (!open) return;
    if (isLoaded && isSignedIn && user) {
      // eslint-disable-next-line react-hooks/set-state-in-effect
      setName(
        user.fullName ??
          [user.firstName, user.lastName].filter(Boolean).join(" ") ??
          ""
      );
      setEmail(user.primaryEmailAddress?.emailAddress ?? "");
    }
  }, [open, isLoaded, isSignedIn, user]);

  function onSubmit(e: FormEvent<HTMLFormElement>) {
    e.preventDefault();
    if (state.kind === "submitting") return;

    setState({ kind: "submitting" });
    startTransition(async () => {
      try {
        const ack = await sendInquiry(artworkId, {
          name: name.trim(),
          // For signed-in users, body email is ignored server-side, but we
          // still pass it for symmetry. For anonymous it's required.
          email: email.trim() || undefined,
          message: message.trim(),
          budget_range: budget.trim() || undefined,
        });
        setState({ kind: "success", ack });
      } catch (e) {
        setState({
          kind: "error",
          message: toUserMessage(
            e,
            "Couldn't send your inquiry. Check your details and try again.",
            { surface: "inquiry-modal" },
          ),
        });
      }
    });
  }

  return (
    <Dialog.Root open={open} onOpenChange={handleOpenChange}>
      <Dialog.Portal>
        <Dialog.Overlay className="fixed inset-0 bg-foreground/30 backdrop-blur-sm z-40" />
        <Dialog.Content
          className="fixed left-1/2 top-1/2 -translate-x-1/2 -translate-y-1/2 z-50 w-[92vw] max-w-md bg-surface border border-border p-6 shadow-2xl"
          aria-describedby={undefined}
        >
          <Dialog.Title className="font-serif text-2xl mb-1">
            Inquire
          </Dialog.Title>
          <p className="text-xs text-muted mb-4">
            This goes directly to {artistName}. You&apos;ll hear back from
            them — we don&apos;t take a cut.
          </p>

          {state.kind === "success" ? (
            state.ack.status === "delivered" ? (
              <div className="py-4">
                <p className="text-sm">Sent. {artistName} will be in touch.</p>
                <button
                  type="button"
                  onClick={() => onOpenChange(false)}
                  className="mt-6 w-full py-2 border border-border text-sm hover:bg-foreground hover:text-background"
                >
                  Close
                </button>
              </div>
            ) : (
              <div className="py-4">
                <p className="text-sm">
                  Check your inbox at <strong>{email}</strong>
                  {" "}for a one-click confirm link. We&apos;ll deliver the
                  message to {artistName} after you confirm.
                </p>
                {state.ack.debug_verification_token && (
                  <p className="mt-4 text-xs text-muted">
                    Dev-only: visit{" "}
                    <a
                      href={`/inquiries/verify/${state.ack.debug_verification_token}`}
                      className="underline"
                    >
                      /inquiries/verify/{state.ack.debug_verification_token.slice(0, 8)}…
                    </a>{" "}
                    to simulate clicking the email link.
                  </p>
                )}
              </div>
            )
          ) : (
            <form onSubmit={onSubmit} className="space-y-3">
              <Field
                label="Name"
                value={name}
                onChange={setName}
                required
                maxLength={120}
              />
              <Field
                label="Email"
                type="email"
                value={email}
                onChange={setEmail}
                required={!isSignedIn}
                readOnly={isSignedIn ?? false}
                helper={
                  isSignedIn
                    ? "Verified via your account"
                    : "We'll email you a single-click confirmation."
                }
              />
              <FieldTextarea
                label="Message"
                value={message}
                onChange={setMessage}
                required
                rows={5}
                maxLength={4000}
              />
              <Field
                label="Budget (optional)"
                value={budget}
                onChange={setBudget}
                placeholder="e.g. $500–$1,000, open"
              />

              {state.kind === "error" && (
                <p className="text-sm text-foreground bg-background border border-border p-3">
                  {state.message}
                </p>
              )}

              <div className="pt-2 flex gap-2">
                <button
                  type="button"
                  onClick={() => onOpenChange(false)}
                  className="px-4 py-2 text-sm border border-border hover:bg-background"
                >
                  Cancel
                </button>
                <button
                  type="submit"
                  disabled={
                    isPending || !name.trim() || !message.trim() ||
                    (!isSignedIn && !email.trim())
                  }
                  className="flex-1 px-4 py-2 text-sm bg-foreground text-background disabled:opacity-40"
                >
                  {isPending ? "Sending…" : "Send inquiry"}
                </button>
              </div>
            </form>
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

function Field({
  label,
  value,
  onChange,
  type = "text",
  required,
  readOnly,
  placeholder,
  maxLength,
  helper,
}: {
  label: string;
  value: string;
  onChange: (v: string) => void;
  type?: string;
  required?: boolean;
  readOnly?: boolean;
  placeholder?: string;
  maxLength?: number;
  helper?: string;
}) {
  return (
    <label className="block">
      <span className="block text-xs text-muted mb-1">{label}</span>
      <input
        type={type}
        value={value}
        onChange={(e) => onChange(e.target.value)}
        required={required}
        readOnly={readOnly}
        placeholder={placeholder}
        maxLength={maxLength}
        className="w-full bg-background border border-border px-3 py-2 text-sm focus:outline-none focus:border-foreground read-only:opacity-70"
      />
      {helper && <span className="block text-xs text-muted mt-1">{helper}</span>}
    </label>
  );
}

function FieldTextarea({
  label,
  value,
  onChange,
  required,
  rows,
  maxLength,
}: {
  label: string;
  value: string;
  onChange: (v: string) => void;
  required?: boolean;
  rows?: number;
  maxLength?: number;
}) {
  return (
    <label className="block">
      <span className="block text-xs text-muted mb-1">{label}</span>
      <textarea
        value={value}
        onChange={(e) => onChange(e.target.value)}
        required={required}
        rows={rows}
        maxLength={maxLength}
        className="w-full bg-background border border-border px-3 py-2 text-sm focus:outline-none focus:border-foreground resize-y"
      />
    </label>
  );
}

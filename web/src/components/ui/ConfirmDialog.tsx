"use client";

/**
 * Promise-based confirm dialog (T-071) — replaces `window.confirm`.
 *
 * Wraps Radix's AlertDialog primitive (which handles focus trapping,
 * Escape-to-cancel, screen-reader semantics) behind a hook with the
 * same async-imperative ergonomics native `confirm()` has:
 *
 *   const confirm = useConfirm();
 *   const ok = await confirm({
 *     title: "Publish without dimensions?",
 *     description: "Buyers won't be able to filter your work by size.",
 *     confirmLabel: "Publish anyway",
 *   });
 *   if (!ok) return;
 *
 * One `<ConfirmDialogProvider>` lives at the app root (already mounted
 * from `app/layout.tsx`). Consumers anywhere below it use the hook and
 * never touch the underlying state directly — the goal is that asking
 * a yes/no question stays a one-line ergonomic affair, not a state
 * machine in every consumer.
 *
 * See `docs/ui-patterns.md` for when to reach for this vs an inline
 * banner vs a toast. ESLint bans `window.confirm`/`alert`/`prompt` so
 * it can't drift.
 */

import { createContext, useCallback, useContext, useRef, useState } from "react";
import * as AlertDialog from "@radix-ui/react-alert-dialog";

export interface ConfirmOptions {
  title: string;
  /** Optional supporting copy. Rendered as the AlertDialog description. */
  description?: string;
  /** Defaults to "Confirm". */
  confirmLabel?: string;
  /** Defaults to "Cancel". */
  cancelLabel?: string;
  /**
   * Renders the confirm button in destructive styling (red). Use for
   * delete / archive / discard flows. Default false.
   */
  destructive?: boolean;
}

type ConfirmFn = (opts: ConfirmOptions) => Promise<boolean>;

const ConfirmContext = createContext<ConfirmFn | null>(null);

interface PendingState {
  opts: ConfirmOptions;
  resolve: (ok: boolean) => void;
}

export function ConfirmDialogProvider({ children }: { children: React.ReactNode }) {
  const [pending, setPending] = useState<PendingState | null>(null);

  // The resolver is held in a ref so an in-flight prompt that's
  // cancelled by the Escape key (which triggers onOpenChange(false))
  // can still settle the promise without a stale closure.
  const resolverRef = useRef<((ok: boolean) => void) | null>(null);

  const confirm = useCallback<ConfirmFn>((opts) => {
    return new Promise((resolve) => {
      resolverRef.current = resolve;
      setPending({ opts, resolve });
    });
  }, []);

  function settle(ok: boolean) {
    const resolve = resolverRef.current;
    resolverRef.current = null;
    setPending(null);
    // T-072 fix — defer the resolver by a tick. When this confirm is
    // nested inside another Radix Dialog (the typical case: edit
    // modal asks for confirmation), both the AlertDialog and the
    // parent Dialog manage body's scroll-lock + pointer-events. If
    // the caller fires `onClose()` synchronously after `await
    // confirm(...)` returns, the parent Dialog starts unmounting in
    // the same React tick as the AlertDialog is still tearing down —
    // the two ref-counted cleanups race and body ends up with
    // `pointer-events: none` stuck. The next tick is enough for the
    // inner cleanup to flush before the outer one starts.
    // See https://github.com/radix-ui/primitives/issues/2122 for the
    // upstream bug; this microtask defer is the common workaround.
    queueMicrotask(() => resolve?.(ok));
  }

  return (
    <ConfirmContext.Provider value={confirm}>
      {children}
      <AlertDialog.Root
        open={pending !== null}
        onOpenChange={(open) => {
          // Closing via Escape / overlay-click resolves false.
          if (!open && resolverRef.current) settle(false);
        }}
      >
        <AlertDialog.Portal>
          <AlertDialog.Overlay className="fixed inset-0 bg-foreground/30 backdrop-blur-sm z-50" />
          <AlertDialog.Content
            className="fixed top-1/2 left-1/2 -translate-x-1/2 -translate-y-1/2 z-50 w-[92vw] max-w-md bg-surface border border-border p-6 shadow-lg focus:outline-none"
          >
            <AlertDialog.Title className="font-serif text-xl mb-2">
              {pending?.opts.title}
            </AlertDialog.Title>
            {pending?.opts.description ? (
              <AlertDialog.Description className="text-sm text-muted mb-5">
                {pending.opts.description}
              </AlertDialog.Description>
            ) : (
              // AlertDialog requires Description for a11y; render an empty
              // one (sr-only) when the caller didn't supply copy.
              <AlertDialog.Description className="sr-only">
                {pending?.opts.title}
              </AlertDialog.Description>
            )}
            <div className="flex justify-end gap-2">
              <AlertDialog.Cancel asChild>
                <button
                  type="button"
                  onClick={() => settle(false)}
                  className="px-3 py-1.5 text-sm border border-border bg-background hover:bg-surface"
                >
                  {pending?.opts.cancelLabel ?? "Cancel"}
                </button>
              </AlertDialog.Cancel>
              <AlertDialog.Action asChild>
                <button
                  type="button"
                  onClick={() => settle(true)}
                  className={[
                    "px-3 py-1.5 text-sm border",
                    pending?.opts.destructive
                      ? "border-red-600 bg-red-600 text-white hover:bg-red-700"
                      : "border-foreground bg-foreground text-background hover:bg-foreground/90",
                  ].join(" ")}
                >
                  {pending?.opts.confirmLabel ?? "Confirm"}
                </button>
              </AlertDialog.Action>
            </div>
          </AlertDialog.Content>
        </AlertDialog.Portal>
      </AlertDialog.Root>
    </ConfirmContext.Provider>
  );
}

/**
 * Hook to fire a confirm prompt. Returns a promise that resolves to
 * `true` (confirm) or `false` (cancel / Escape / overlay click).
 *
 * Throws synchronously if called outside `<ConfirmDialogProvider>`, so
 * misuse fails loudly during dev rather than silently no-op'ing.
 */
export function useConfirm(): ConfirmFn {
  const ctx = useContext(ConfirmContext);
  if (!ctx) {
    throw new Error(
      "useConfirm() must be used inside <ConfirmDialogProvider>. " +
        "Provider is mounted at app/layout.tsx — was the call moved outside it?",
    );
  }
  return ctx;
}

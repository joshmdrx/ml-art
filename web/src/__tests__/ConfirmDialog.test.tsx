// @vitest-environment happy-dom

/**
 * `useConfirm()` + `<ConfirmDialogProvider>` (T-071).
 *
 * The hook is async-imperative (returns a Promise<boolean>). The
 * confirm vs cancel vs close-via-escape branches all need to resolve
 * the promise correctly — otherwise a forgotten promise hangs the
 * caller forever. These tests pin every branch.
 *
 * Radix portals into document.body, so `screen.getByRole` finds the
 * dialog buttons even though they live outside the provider's tree.
 */

import { describe, expect, it } from "vitest";
import { render, screen, act } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { ConfirmDialogProvider, useConfirm } from "@/components/ui/ConfirmDialog";

/** Test harness — clicking the button fires confirm() and writes the
 *  resolved boolean into a visible element so the assertion is on
 *  observable DOM state, not internal hook state. */
function Harness({ opts }: { opts: Parameters<ReturnType<typeof useConfirm>>[0] }) {
  const confirm = useConfirm();
  return (
    <div>
      <button
        type="button"
        onClick={async () => {
          const ok = await confirm(opts);
          // Stash result in a data-attr so the test can read it.
          document.body.setAttribute("data-result", String(ok));
        }}
      >
        ask
      </button>
    </div>
  );
}

function setup(opts: Parameters<ReturnType<typeof useConfirm>>[0]) {
  document.body.removeAttribute("data-result");
  const user = userEvent.setup();
  render(
    <ConfirmDialogProvider>
      <Harness opts={opts} />
    </ConfirmDialogProvider>,
  );
  return { user };
}

describe("useConfirm + ConfirmDialogProvider", () => {
  it("resolves true when the confirm button is clicked", async () => {
    const { user } = setup({ title: "Delete this artwork?", confirmLabel: "Delete" });

    await user.click(screen.getByText("ask"));
    // Dialog opens — title + button visible. Radix doubles the title
    // into the sr-only Description when no description is supplied,
    // so we target the heading role specifically.
    expect(screen.getByRole("heading", { name: "Delete this artwork?" })).toBeTruthy();
    await user.click(screen.getByRole("button", { name: "Delete" }));

    // The harness wrote the resolved boolean back to body.
    expect(document.body.getAttribute("data-result")).toBe("true");
  });

  it("resolves false when the cancel button is clicked", async () => {
    const { user } = setup({ title: "Cancel test", cancelLabel: "Nope" });

    await user.click(screen.getByText("ask"));
    await user.click(screen.getByRole("button", { name: "Nope" }));

    expect(document.body.getAttribute("data-result")).toBe("false");
  });

  it("resolves false when the user presses Escape", async () => {
    const { user } = setup({ title: "Escape test" });

    await user.click(screen.getByText("ask"));
    await user.keyboard("{Escape}");

    // happy-dom + Radix sometimes settle this on the next microtask.
    await act(async () => {
      await Promise.resolve();
    });
    expect(document.body.getAttribute("data-result")).toBe("false");
  });

  it("applies destructive styling when requested", async () => {
    const { user } = setup({
      title: "Destructive",
      confirmLabel: "Delete",
      destructive: true,
    });

    await user.click(screen.getByText("ask"));
    const confirmBtn = screen.getByRole("button", { name: "Delete" });
    // Destructive uses red border + bg. We only need to know it's
    // *different* from the default; pin one stable class.
    expect(confirmBtn.className).toContain("border-red-600");
  });

  it("throws when used outside the provider", () => {
    // The hook calls useContext; without a provider it returns null
    // and the hook should throw a clear message.
    function Bad() {
      const _confirm = useConfirm();
      return null;
    }
    // React 18+ surfaces the error as a thrown render error.
    expect(() => render(<Bad />)).toThrow(/ConfirmDialogProvider/);
  });
});

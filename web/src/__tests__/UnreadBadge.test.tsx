// @vitest-environment happy-dom

/**
 * `<UnreadBadge>` (T-074) — tiny but the contract matters: 0 hides,
 * 1–9 shows the number, 10+ caps at "9+", aria-label always present
 * for screen readers.
 */

import { describe, expect, it } from "vitest";
import { render, screen } from "@testing-library/react";
import { UnreadBadge } from "@/components/ui/UnreadBadge";

describe("UnreadBadge", () => {
  it("renders the count when > 0", () => {
    render(<UnreadBadge count={3} label="3 unread inquiries" />);
    const badge = screen.getByLabelText("3 unread inquiries");
    expect(badge.textContent).toBe("3");
  });

  it("renders nothing for count 0", () => {
    const { container } = render(
      <UnreadBadge count={0} label="0 unread" />,
    );
    expect(container.firstChild).toBeNull();
  });

  it("renders nothing for negative count (defensive)", () => {
    const { container } = render(
      <UnreadBadge count={-1} label="negative" />,
    );
    expect(container.firstChild).toBeNull();
  });

  it("caps display at '9+' for counts above 9", () => {
    render(<UnreadBadge count={47} label="47 unread inquiries" />);
    const badge = screen.getByLabelText("47 unread inquiries");
    expect(badge.textContent).toBe("9+");
  });

  it("shows 9 (not 9+) for exactly nine", () => {
    render(<UnreadBadge count={9} label="9 unread" />);
    expect(screen.getByLabelText("9 unread").textContent).toBe("9");
  });

  it("shows 9+ for exactly ten", () => {
    render(<UnreadBadge count={10} label="10 unread" />);
    expect(screen.getByLabelText("10 unread").textContent).toBe("9+");
  });
});

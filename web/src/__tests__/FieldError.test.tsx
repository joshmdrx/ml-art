// @vitest-environment happy-dom

/**
 * `<FieldError>` (T-071) — tiny component, but the API contract
 * (null when empty, `role="alert"` for SR, red styling) is what
 * downstream consumers rely on.
 */

import { describe, expect, it } from "vitest";
import { render, screen } from "@testing-library/react";
import { FieldError } from "@/components/ui/FieldError";

describe("FieldError", () => {
  it("renders the message with role=alert when given one", () => {
    render(<FieldError message="Width and height are both required." />);
    const alert = screen.getByRole("alert");
    expect(alert.textContent).toBe("Width and height are both required.");
  });

  it("renders nothing for null", () => {
    const { container } = render(<FieldError message={null} />);
    expect(container.firstChild).toBeNull();
  });

  it("renders nothing for undefined", () => {
    const { container } = render(<FieldError message={undefined} />);
    expect(container.firstChild).toBeNull();
  });

  it("renders nothing for the empty string", () => {
    const { container } = render(<FieldError message="" />);
    // Falsy → null branch. Important: this matches the pattern call
    // sites use (`message={fieldError}` where fieldError === "" wouldn't
    // happen but we treat falsy uniformly).
    expect(container.firstChild).toBeNull();
  });
});

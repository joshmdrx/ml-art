/**
 * Vitest global setup — happy-dom test files only.
 *
 * @testing-library/react auto-cleans after each test in Jest, but
 * Vitest doesn't wire that up automatically. Without this hook,
 * previous render trees stay in document.body and subsequent
 * `screen.getByText` calls find multiple matches and throw.
 *
 * Pure-function tests (environment: "node") don't need this — they
 * never render. The file is loaded conditionally via setupFiles in
 * vitest.config.ts; the cleanup-import is a no-op when no React
 * trees were mounted.
 */

import { afterEach } from "vitest";
import { cleanup } from "@testing-library/react";

afterEach(() => {
  cleanup();
});

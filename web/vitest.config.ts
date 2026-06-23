import { defineConfig } from "vitest/config";
import path from "node:path";

export default defineConfig({
  test: {
    include: ["src/__tests__/**/*.test.ts", "src/__tests__/**/*.test.tsx"],
    // Default is `node` for pure-function units. `.test.tsx` files that
    // render React opt into happy-dom via `// @vitest-environment
    // happy-dom` at the top of the file — see ConfirmDialog.test.tsx.
    environment: "node",
    // Auto-cleanup between tests for any file that mounted a React
    // tree via @testing-library/react. No-op for pure-function tests.
    setupFiles: ["./vitest.setup.ts"],
  },
  resolve: {
    alias: {
      "@": path.resolve(__dirname, "./src"),
    },
  },
});

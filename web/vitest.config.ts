import { defineConfig } from "vitest/config";
import path from "node:path";

export default defineConfig({
  test: {
    include: ["src/__tests__/**/*.test.ts", "src/__tests__/**/*.test.tsx"],
    environment: "node", // pure-function units; DOM tests can switch per-file
  },
  resolve: {
    alias: {
      "@": path.resolve(__dirname, "./src"),
    },
  },
});

import { defineConfig, globalIgnores } from "eslint/config";
import nextVitals from "eslint-config-next/core-web-vitals";
import nextTs from "eslint-config-next/typescript";

const eslintConfig = defineConfig([
  ...nextVitals,
  ...nextTs,
  // Override default ignores of eslint-config-next.
  globalIgnores([
    // Default ignores of eslint-config-next:
    ".next/**",
    "out/**",
    "build/**",
    "next-env.d.ts",
  ]),
  {
    // Project-wide rules — see `CONTRIBUTING.md` § Code conventions.
    rules: {
      // Bare `console.error` is rejected; use `reportError(err, ctx)` from
      // `lib/reportError.ts` so the future Sentry swap is one-file.
      // `console.warn` / `console.info` are still allowed for genuinely
      // non-error logs. See `decisions.md` 2026-05-27 — Error reporter shim.
      "no-console": ["error", { allow: ["warn", "info"] }],
    },
  },
  {
    // `reportError` is the one place a real `console.error` is correct.
    files: ["src/lib/reportError.ts"],
    rules: { "no-console": "off" },
  },
]);

export default eslintConfig;

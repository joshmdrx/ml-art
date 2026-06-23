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
    // OpenNext build artifacts — bundled / minified output, not source.
    // Without this, `pnpm lint` lights up tens of thousands of errors in
    // generated chunks. Added 2026-06-22 with the T-071 lint rules so
    // the new no-restricted-globals findings aren't drowned in noise.
    ".open-next/**",
  ]),
  {
    // Project-wide rules — see `CONTRIBUTING.md` § Code conventions.
    rules: {
      // Bare `console.error` is rejected; use `reportError(err, ctx)` from
      // `lib/reportError.ts` so the future Sentry swap is one-file.
      // `console.warn` / `console.info` are still allowed for genuinely
      // non-error logs. See `decisions.md` 2026-05-27 — Error reporter shim.
      "no-console": ["error", { allow: ["warn", "info"] }],

      // T-071 — native modal globals are banned in favour of
      // `useConfirm()` (yes/no) and `toast.*` (info/success/error).
      // They give consistent styling, focus management, and a11y. See
      // `docs/ui-patterns.md` and `decisions.md` 2026-06-22 (Feedback
      // primitives).
      "no-restricted-globals": [
        "error",
        {
          name: "confirm",
          message:
            "Use useConfirm() from @/components/ui/ConfirmDialog. See docs/ui-patterns.md.",
        },
        {
          name: "alert",
          message:
            "Use toast.success/error/info from sonner, or a styled Dialog. See docs/ui-patterns.md.",
        },
        {
          name: "prompt",
          message:
            "Don't prompt — build the input into the form. See docs/ui-patterns.md.",
        },
      ],
      // Cover the `window.confirm(...)` form too — no-restricted-globals
      // only catches the bare identifier. Same rationale.
      "no-restricted-properties": [
        "error",
        {
          object: "window",
          property: "confirm",
          message:
            "Use useConfirm() from @/components/ui/ConfirmDialog. See docs/ui-patterns.md.",
        },
        {
          object: "window",
          property: "alert",
          message:
            "Use toast.success/error/info from sonner, or a styled Dialog. See docs/ui-patterns.md.",
        },
        {
          object: "window",
          property: "prompt",
          message:
            "Don't prompt — build the input into the form. See docs/ui-patterns.md.",
        },
      ],
    },
  },
  {
    // `reportError` is the one place a real `console.error` is correct.
    files: ["src/lib/reportError.ts"],
    rules: { "no-console": "off" },
  },
]);

export default eslintConfig;

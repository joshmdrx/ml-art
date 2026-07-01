import { defineConfig, devices } from "@playwright/test";
import "dotenv/config";

// Eagerly load .env.local so @clerk/testing's clerkSetup() sees the keys
// without us passing them on every command line.
import { config as loadEnv } from "dotenv";
loadEnv({ path: ".env.local" });
loadEnv({ path: ".env" });

/**
 * Playwright config for ml-art end-to-end tests.
 *
 * The stack must be running before tests start:
 *   1. `docker compose -f ../docker-compose.dev.yml up -d`
 *   2. db migrations applied + seed loaded
 *   3. `cargo run -p api-search` on :9100
 *   4. `pnpm dev` (in ../web) on :3000
 *
 * CI handles all of this in `.github/workflows/e2e.yml`. Locally, just
 * bring everything up by hand or via `make dev`.
 *
 * Auth: a "setup" project runs `auth.setup.ts` once per test run, signs
 * up a fresh Clerk test user, and saves the authenticated browser state
 * to `e2e/.auth/user.json`. The `chromium-authed` project consumes that
 * storage state. Tests choose which project to run under via their
 * filename pattern.
 */
export default defineConfig({
  testDir: "./tests",
  fullyParallel: true,
  retries: process.env.CI ? 2 : 1,
  workers: process.env.CI ? 2 : undefined,
  reporter: [["list"], ["html", { open: "never" }]],
  timeout: 30_000,
  expect: { timeout: 5_000 },

  use: {
    baseURL: process.env.E2E_BASE_URL ?? "http://localhost:3000",
    trace: "retain-on-failure",
    screenshot: "only-on-failure",
    video: "retain-on-failure",
  },

  projects: [
    // 1. One-shot Clerk sign-up (regular user); saves storageState for
    //    the `chromium-authed` project.
    {
      name: "setup",
      testMatch: /auth\.setup\.ts/,
    },

    // 2. One-shot Clerk sign-up (admin user); saves storageState for
    //    the `chromium-admin` project. Requires the API to be running
    //    with `WANDER_ADMIN_EMAIL_ALLOWLIST=-admin+clerk_test@example.com`
    //    (set by `scripts/dev.sh` locally + the e2e workflow in CI).
    {
      name: "admin-setup",
      testMatch: /admin\.setup\.ts/,
    },

    // 3. One-shot Clerk sign-up + onboarding-wizard drive; saves
    //    storageState for the `chromium-artist` project.
    {
      name: "artist-setup",
      testMatch: /artist\.setup\.ts/,
    },

    // 4. Anonymous tests. Default state — fresh browser, no cookies.
    {
      name: "chromium",
      use: { ...devices["Desktop Chrome"] },
      dependencies: ["setup"],
      testIgnore: /.*signed-in.*\.spec\.ts/,
    },

    // 5. Signed-in-as-regular-user tests. Match `*-signed-in*.spec.ts`
    //    but exclude admin + artist variants that route elsewhere.
    {
      name: "chromium-authed",
      use: {
        ...devices["Desktop Chrome"],
        storageState: "e2e/.auth/user.json",
      },
      dependencies: ["setup"],
      testMatch: /.*signed-in.*\.spec\.ts/,
      testIgnore: /.*(admin|artist)-signed-in.*\.spec\.ts/,
    },

    // 6. Signed-in-as-admin tests. Match `*-admin-signed-in*.spec.ts`.
    {
      name: "chromium-admin",
      use: {
        ...devices["Desktop Chrome"],
        storageState: "e2e/.auth/admin.json",
      },
      dependencies: ["admin-setup"],
      testMatch: /.*admin-signed-in.*\.spec\.ts/,
    },

    // 7. Signed-in-as-onboarded-artist tests. Match
    //    `*-artist-signed-in*.spec.ts`. Setup drives the whole
    //    onboarding wizard so downstream specs can drive /studio
    //    without redirect-to-/onboarding friction.
    {
      name: "chromium-artist",
      use: {
        ...devices["Desktop Chrome"],
        storageState: "e2e/.auth/artist.json",
      },
      dependencies: ["artist-setup"],
      testMatch: /.*artist-signed-in.*\.spec\.ts/,
    },
  ],
});

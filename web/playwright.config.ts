import { defineConfig, devices } from "@playwright/test";

/**
 * Playwright config for the underwrap admin SPA.
 *
 * Tests live in `web/e2e/`. Each spec gets its own `backend` fixture
 * (see `e2e/fixtures.ts`) that spawns a real `underwrap-backendd`
 * binary on a random TLS port, against a temp store. The base URL is
 * per-test and exposed via `backend.baseUrl` rather than a global
 * `use.baseURL`, because there isn't a single shared server.
 *
 * Prerequisites (run inside `nix develop`):
 *   - `cargo build -p backend-backendd`  (writes target/debug/underwrap-backendd)
 *   - `npm run build`                    (writes web/dist/)
 *
 * `npm run test:e2e` does the SPA build for you; the backend binary is
 * the user's responsibility — the fixture errors with a clear message
 * if it's missing.
 */
export default defineConfig({
  testDir: "./e2e",
  fullyParallel: true,
  forbidOnly: !!process.env.CI,
  retries: process.env.CI ? 1 : 0,
  reporter: process.env.CI ? "line" : "list",
  timeout: 30_000,
  expect: { timeout: 5_000 },

  use: {
    // Self-signed certs per worker — Playwright must not reject them.
    ignoreHTTPSErrors: true,
    trace: "retain-on-failure",
    screenshot: "only-on-failure",
  },

  projects: [
    {
      // Ticket 252: 1280-wide desktop project — the default scope
      // for every test that isn't viewport-specific.
      name: "desktop-1280",
      use: {
        ...devices["Desktop Chrome"],
        viewport: { width: 1280, height: 800 },
      },
      // Mobile- and tablet-specific specs run under their own
      // projects; exclude them from the desktop run so the
      // burger-menu assertions don't fail against a desktop
      // viewport (the sidebar is visible there instead of the
      // drawer).
      testIgnore: /.*\.(mobile|tablet)\.spec\.ts/,
    },
    {
      // Tablet project (768×1024 portrait) — picks up the
      // `*.tablet.spec.ts` files plus serves as a viewport floor
      // for table → card transitions.
      name: "tablet-768",
      use: {
        ...devices["iPad (gen 7)"],
      },
      testMatch: /.*\.tablet\.spec\.ts/,
    },
    {
      // Spot-checks mobile drawer + form layout on a small viewport.
      // Kept in the default run because it's cheap and the 220/250
      // stories explicitly call out mobile support.
      name: "mobile-375",
      use: { ...devices["iPhone SE (3rd gen)"] ?? devices["iPhone SE"] },
      testMatch: /.*\.mobile\.spec\.ts/,
    },
  ],
});

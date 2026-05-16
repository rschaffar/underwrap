/**
 * Playwright fixtures. The `backend` fixture spawns a fresh
 * `underwrap-backendd` per test, against a temp store with a random
 * TLS port (see `./backendd.ts`). Tests reach the SPA at
 * `backend.baseUrl`.
 *
 * Two variants:
 *   - `test`         — empty backend, useful for the bootstrap flow.
 *   - `loggedOutTest` — backend that already has an admin user. Tests
 *                       hit `/login` to exercise the login UI.
 */
import { test as base, type Page } from "@playwright/test";
import { startBackend, type BackendInstance } from "./backendd";

export const ADMIN_USERNAME = "alice";
export const ADMIN_PASSWORD = "correct-horse-battery";

interface Fixtures {
  backend: BackendInstance;
  backendWithAdmin: BackendInstance;
}

export const test = base.extend<Fixtures>({
  backend: async ({}, use) => {
    const instance = await startBackend();
    await use(instance);
    await instance.cleanup();
  },
  backendWithAdmin: async ({}, use) => {
    const instance = await startBackend({
      withAdmin: { username: ADMIN_USERNAME, password: ADMIN_PASSWORD },
    });
    await use(instance);
    await instance.cleanup();
  },
});

export { expect } from "@playwright/test";

/** Convenience: log in via the UI on a `backendWithAdmin` setup. */
export async function loginViaUi(
  page: Page,
  baseUrl: string,
  username = ADMIN_USERNAME,
  password = ADMIN_PASSWORD,
): Promise<void> {
  await page.goto(`${baseUrl}/login`);
  await page.getByLabel("Username").fill(username);
  await page.getByLabel("Password").fill(password);
  await page.getByRole("button", { name: /sign in/i }).click();
  // Successful login lands on the dashboard.
  await page.waitForURL(`${baseUrl}/`);
}

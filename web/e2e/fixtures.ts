/**
 * Playwright fixtures. The `backend` fixture spawns a fresh
 * `underwrap-backend` per test, against a temp store with a random
 * TLS port (see `./backend.ts`). Tests reach the SPA at
 * `backend.baseUrl`.
 *
 * Two variants:
 *   - `test`         — empty backend, useful for the bootstrap flow.
 *   - `loggedOutTest` — backend that already has an admin user. Tests
 *                       hit `/login` to exercise the login UI.
 */
import { test as base, type Page } from "@playwright/test";
import { startBackend, type BackendInstance } from "./backend";

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
  // Successful login lands on `/`, which redirects to the Secrets page.
  await page.waitForURL(
    (url) => url.pathname === "/" || url.pathname.startsWith("/secrets"),
  );
}

/**
 * Drive an admin mutation from inside the page. The CSRF cookie is
 * HttpOnly (not JS-readable), so — exactly like the SPA client — this
 * captures the `X-CSRF-Token` synchronizer token from a priming admin
 * GET response and replays it. Does not throw on non-2xx; returns the
 * status and raw body so callers can assert expected error codes.
 */
export async function adminMutate(
  page: Page,
  method: "POST" | "PUT" | "DELETE",
  path: string,
  body?: unknown,
): Promise<{ status: number; body: string }> {
  return page.evaluate(
    async ({ method, path, body }) => {
      const prime = await fetch("/v1/admin/stores", { credentials: "include" });
      if (!prime.ok) {
        throw new Error(`csrf prime GET failed ${prime.status} — login first`);
      }
      const csrf = prime.headers.get("X-CSRF-Token");
      if (!csrf) throw new Error("no X-CSRF-Token header on admin response");
      const res = await fetch(path, {
        method,
        credentials: "include",
        headers: { "Content-Type": "application/json", "X-CSRF-Token": csrf },
        body: body === undefined ? undefined : JSON.stringify(body),
      });
      return { status: res.status, body: await res.text() };
    },
    { method, path, body },
  );
}

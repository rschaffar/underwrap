/**
 * Baseline auth e2e — runs against a freshly-spawned backend per
 * test. The scenarios cover the must-not-regress paths:
 *
 *   1. fresh backend → offline setup guidance; network bootstrap rejected
 *   2. login with the wrong password → inline error
 *   3. hard-navigation logout invalidates the server-side session
 *   4. failed hard-navigation logout stays visible and can be retried
 *   5. CSRF token is attached on POSTs the SPA initiates
 *   6. an unauthenticated API call redirects the user to /login
 */
import { request as playwrightRequest } from "@playwright/test";
import {
  test,
  expect,
  loginViaUi,
  ADMIN_USERNAME,
  ADMIN_PASSWORD,
} from "./fixtures";

async function replayProtectedRequest(
  baseUrl: string,
  session: { name: string; value: string },
): Promise<{ status: number; code: string | undefined }> {
  const replay = await playwrightRequest.newContext({
    ignoreHTTPSErrors: true,
    extraHTTPHeaders: {
      Cookie: `${session.name}=${session.value}`,
    },
  });
  try {
    const response = await replay.get(`${baseUrl}/v1/admin/stores`);
    const body = (await response.json()) as { code?: string };
    return { status: response.status(), code: body.code };
  } finally {
    await replay.dispose();
  }
}

test("fresh backend requires offline setup and rejects network bootstrap", async ({
  page,
  backend,
}) => {
  await page.goto(`${backend.baseUrl}/`);
  await expect(page).toHaveURL(/\/bootstrap(\?|$)/);
  await expect(
    page.getByRole("heading", { name: /first-time setup required/i }),
  ).toBeVisible();
  await expect(page.getByText(/bootstrap-user --username/i)).toBeVisible();
  await expect(page.getByLabel("Username")).toHaveCount(0);
  await expect(page.getByLabel(/password/i)).toHaveCount(0);

  const hostile = await page.request.post(
    `${backend.baseUrl}/v1/admin/auth/bootstrap`,
    {
      headers: { Origin: backend.baseUrl },
      data: { username: ADMIN_USERNAME, password: ADMIN_PASSWORD },
    },
  );
  expect([404, 405]).toContain(hostile.status());

  const status = await page.request.get(
    `${backend.baseUrl}/v1/admin/auth/bootstrap/status`,
  );
  expect(await status.json()).toEqual({ bootstrap_required: true });
});

test("login with wrong password shows inline error", async ({
  page,
  backendWithAdmin,
}) => {
  await page.goto(`${backendWithAdmin.baseUrl}/login`);
  await page.getByLabel("Username").fill(ADMIN_USERNAME);
  await page.getByLabel("Password").fill("definitely-wrong-password");
  await page.getByRole("button", { name: /sign in/i }).click();

  await expect(
    page.getByText(/invalid username or password/i),
  ).toBeVisible();
  // Still on /login — no redirect on failure.
  await expect(page).toHaveURL(/\/login/);
});

test("logout invalidates the session server-side", async ({
  page,
  backendWithAdmin,
}) => {
  await loginViaUi(page, backendWithAdmin.baseUrl);
  // Capture the session cookie so we can replay it after logout and
  // see the server reject it.
  const cookies = await page.context().cookies(backendWithAdmin.baseUrl);
  const session = cookies.find((c) => c.name === "__Host-underwrap-session");
  expect(session).toBeDefined();

  // Load /logout as a fresh document so the SPA starts without its
  // module-scoped CSRF token and must prime before revoking the session.
  await page.goto(`${backendWithAdmin.baseUrl}/logout`);
  await page.waitForURL(/\/login(\?|$)/);

  // Replay the old cookie in a fresh request context. `/whoami` is
  // intentionally tolerant, so use a protected endpoint to pin the
  // canonical rejection after backend session destruction.
  const replayed = await replayProtectedRequest(
    backendWithAdmin.baseUrl,
    session!,
  );
  expect(replayed).toEqual({ status: 401, code: "unauthorized" });
});

test("failed hard-navigation logout remains visible and retryable", async ({
  page,
  backendWithAdmin,
}) => {
  await loginViaUi(page, backendWithAdmin.baseUrl);
  const cookies = await page.context().cookies(backendWithAdmin.baseUrl);
  const session = cookies.find((c) => c.name === "__Host-underwrap-session");
  expect(session).toBeDefined();

  await page.route(
    "**/v1/admin/auth/logout",
    async (route) => {
      await route.fulfill({
        status: 500,
        contentType: "application/json",
        body: JSON.stringify({
          code: "logout_failed",
          message: "The request failed.",
        }),
      });
    },
    { times: 1 },
  );

  const [failedLogout] = await Promise.all([
    page.waitForRequest(
      (request) =>
        request.url().endsWith("/v1/admin/auth/logout") &&
        request.method() === "POST",
    ),
    page.goto(`${backendWithAdmin.baseUrl}/logout`),
  ]);

  expect(
    failedLogout.headers()["x-csrf-token"],
    "hard-navigation logout must prime and send a CSRF token",
  ).toBeTruthy();
  await expect(page).toHaveURL(/\/logout(\?|$)/);
  await expect(
    page.getByRole("heading", { name: "Sign out failed" }),
  ).toBeVisible();
  await expect(
    page.getByRole("button", { name: "Retry sign out" }),
  ).toBeVisible();

  const stillAuthenticated = await page.request.get(
    `${backendWithAdmin.baseUrl}/v1/admin/stores`,
  );
  expect(stillAuthenticated.status()).toBe(200);

  const [retryPrime, retryLogout] = await Promise.all([
    page.waitForRequest(
      (request) =>
        request.url().endsWith("/v1/admin/auth/whoami") &&
        request.method() === "GET",
    ),
    page.waitForResponse(
      (response) =>
        response.url().endsWith("/v1/admin/auth/logout") &&
        response.request().method() === "POST",
    ),
    page.getByRole("button", { name: "Retry sign out" }).click(),
  ]);
  expect(retryPrime.method()).toBe("GET");
  expect(retryLogout.status()).toBe(204);
  await expect(page).toHaveURL(/\/login(\?|$)/);

  const replayed = await replayProtectedRequest(
    backendWithAdmin.baseUrl,
    session!,
  );
  expect(replayed).toEqual({ status: 401, code: "unauthorized" });
});

test("CSRF token is attached on state-changing requests", async ({
  page,
  backendWithAdmin,
}) => {
  await loginViaUi(page, backendWithAdmin.baseUrl);

  // The SPA uses a synchronizer-token scheme: it captures `X-CSRF-Token`
  // from admin responses and replays it on the next state-changing
  // request. (The `__Host-underwrap-csrf` cookie is HttpOnly and is not the
  // value sent in the header.) Sign out via the client-side link and
  // assert the logout POST carried a non-empty token.
  const [request] = await Promise.all([
    page.waitForRequest(
      (r) =>
        r.url().endsWith("/v1/admin/auth/logout") && r.method() === "POST",
    ),
    page.getByRole("link", { name: /sign out/i }).click(),
  ]);

  const sentToken = request.headers()["x-csrf-token"];
  expect(
    sentToken,
    "state-changing requests must carry the synchronizer CSRF token",
  ).toBeTruthy();
});

test("unauthenticated visit redirects to /login with the original target", async ({
  page,
  backendWithAdmin,
}) => {
  // No session yet → root guard sees `authenticated: false` and
  // bootstrap_required: false → redirects to /login with `?next=`.
  await page.goto(`${backendWithAdmin.baseUrl}/`);
  await expect(page).toHaveURL(/\/login(\?.*)?$/);
  const url = new URL(page.url());
  expect(url.searchParams.get("next")).toBeTruthy();
});

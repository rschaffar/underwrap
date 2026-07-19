/**
 * Baseline auth e2e — runs against a freshly-spawned backend per
 * test. The scenarios cover the must-not-regress paths:
 *
 *   1. fresh backend → offline setup guidance; network bootstrap rejected
 *   2. login with the wrong password → inline error
 *   3. logout invalidates the server-side session
 *   4. CSRF token is attached on POSTs the SPA initiates
 *   5. an unauthenticated API call redirects the user to /login
 */
import { request as playwrightRequest } from "@playwright/test";
import { test, expect, loginViaUi, ADMIN_USERNAME, ADMIN_PASSWORD } from "./fixtures";

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

  // Sign out via the AppShell control. It is a client-side `<Link to=
  // "/logout">`, so the logout route's `postLogout()` runs in the live
  // SPA with the CSRF synchronizer token already primed (a fresh
  // `page.goto("/logout")` would start with no primed token).
  await page.getByRole("link", { name: /sign out/i }).click();
  await page.waitForURL(/\/login(\?|$)/);

  // Replay the old session cookie via a *fresh* request context (no
  // cookie jar) against a protected endpoint. The acceptance criterion
  // is that the next request to `/v1/admin/...` after logout returns
  // 401 — we exercise it on `/stores` because `/whoami` is
  // intentionally tolerant (it returns 200 with no username rather than
  // 401, so the SPA can probe without alarming devtools). Logout
  // destroys the session server-side, so the replayed cookie resolves
  // to no session and the layer answers `unauthorized`.
  const replay = await playwrightRequest.newContext({
    ignoreHTTPSErrors: true,
    extraHTTPHeaders: {
      Cookie: `${session!.name}=${session!.value}`,
    },
  });
  const protectedRes = await replay.get(
    `${backendWithAdmin.baseUrl}/v1/admin/stores`,
  );
  expect(protectedRes.status()).toBe(401);
  const body = await protectedRes.json();
  expect(body.code).toBe("unauthorized");
  await replay.dispose();
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

/**
 * Baseline auth e2e — runs against a freshly-spawned backendd per
 * test. The scenarios cover the must-not-regress paths called out in
 * ticket 224:
 *
 *   1. bootstrap → login → dashboard
 *   2. login with the wrong password → inline error
 *   3. logout invalidates the server-side session
 *   4. CSRF token is attached on POSTs the SPA initiates
 *   5. an unauthenticated API call redirects the user to /login
 */
import { request as playwrightRequest } from "@playwright/test";
import { test, expect, loginViaUi, ADMIN_USERNAME, ADMIN_PASSWORD } from "./fixtures";

test("bootstrap-then-login happy path", async ({ page, backend }) => {
  // 1) The guard sees an empty operator collection and redirects to
  //    /bootstrap.
  await page.goto(`${backend.baseUrl}/`);
  // The guard redirects to /bootstrap with a `?next=...` echo of the
  // original URL, so anchor on the path only.
  await expect(page).toHaveURL(/\/bootstrap(\?|$)/);
  await expect(
    page.getByRole("heading", { name: /first-time setup/i }),
  ).toBeVisible();

  // 2) Fill the bootstrap form. The page form enforces password >= 12
  //    and confirms match — use a deliberately strong password.
  await page.getByLabel("Username").fill(ADMIN_USERNAME);
  await page.getByLabel(/^password$/i).fill(ADMIN_PASSWORD);
  await page.getByLabel(/confirm password/i).fill(ADMIN_PASSWORD);
  await page.getByRole("button", { name: /create first admin/i }).click();

  // 3) The page should now be on the dashboard (auto-signed-in).
  await page.waitForURL(`${backend.baseUrl}/`);
  await expect(page.getByRole("heading", { name: /dashboard/i })).toBeVisible();

  // 4) The whoami endpoint reflects the new identity.
  const whoami = await page.request.get(
    `${backend.baseUrl}/v1/admin/auth/whoami`,
  );
  expect(whoami.status()).toBe(200);
  const body = await whoami.json();
  expect(body).toMatchObject({
    authenticated: true,
    username: ADMIN_USERNAME,
    role: "admin",
  });

  // 5) A subsequent visit to /bootstrap (or trying to bootstrap again)
  //    must report bootstrap_required: false.
  const status = await page.request.get(
    `${backend.baseUrl}/v1/admin/auth/bootstrap/status`,
  );
  expect(await status.json()).toEqual({ bootstrap_required: false });
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
  const session = cookies.find((c) => c.name === "__Host-uw-session");
  expect(session).toBeDefined();

  // Trigger the logout route (the AppShell's sign-out button delegates
  // to `/logout` which POSTs and clears the cache).
  await page.goto(`${backendWithAdmin.baseUrl}/logout`);
  await page.waitForURL(/\/login(\?|$)/);

  // The cookies have been cleared (Set-Cookie with Max-Age=0).
  const afterLogout = await page
    .context()
    .cookies(backendWithAdmin.baseUrl);
  expect(
    afterLogout.find((c) => c.name === "__Host-uw-session"),
  ).toBeUndefined();

  // Replay the old session cookie via a *fresh* request context (no
  // cookie jar) against a protected endpoint. The acceptance criterion
  // is that the next request to `/v1/admin/...` after logout returns
  // 401 — we exercise it on `/stores` because `/whoami` is
  // intentionally tolerant and returns 200+authenticated:false rather
  // than 401 (so the SPA can probe without alarming devtools).
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
  expect(body.code).toBe("session_revoked");
  await replay.dispose();
});

test("CSRF token is attached on state-changing requests", async ({
  page,
  backendWithAdmin,
}) => {
  await loginViaUi(page, backendWithAdmin.baseUrl);

  // Watch the next POST go out. The SPA's apiFetch reads the CSRF
  // cookie and copies it into X-CSRF-Token; we assert both.
  const csrfCookie = (
    await page.context().cookies(backendWithAdmin.baseUrl)
  ).find((c) => c.name === "__Host-uw-csrf");
  expect(csrfCookie).toBeDefined();

  const [request] = await Promise.all([
    page.waitForRequest(
      (r) =>
        r.url().endsWith("/v1/admin/auth/logout") && r.method() === "POST",
    ),
    page.goto(`${backendWithAdmin.baseUrl}/logout`),
  ]);

  const sentToken = request.headers()["x-csrf-token"];
  expect(sentToken).toBe(csrfCookie!.value);
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

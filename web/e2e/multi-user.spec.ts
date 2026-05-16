/**
 * Multi-user authz e2e (ticket 243).
 *
 * Walks the full grant-aware operator flow against a real backendd:
 *
 *   1. Admin (the fixture's bootstrap user) creates an operator with a
 *      grant to one store.
 *   2. Admin logs out.
 *   3. The operator logs in, sees only their granted store, has no
 *      Users / Stores nav items, and changes their own password from
 *      /profile.
 *   4. The operator logs out and re-logs in with the new password.
 *   5. The operator can't navigate directly to /users (router
 *      redirects to the dashboard) and the admin API returns 403 if
 *      they try anyway.
 */
import {
  test,
  expect,
  loginViaUi,
  ADMIN_USERNAME,
  ADMIN_PASSWORD,
} from "./fixtures";

test("admin creates an operator → operator login restricts what they see and lets them change their password", async ({
  page,
  backendWithAdmin,
}) => {
  await loginViaUi(page, backendWithAdmin.baseUrl);

  // Plant a granted store + an ungranted store. We do this via the
  // admin API rather than the create-store UI because the goal is to
  // exercise the multi-user flow, not the stores CRUD path.
  await page.goto(`${backendWithAdmin.baseUrl}/stores`);
  for (const name of ["granted", "secret"]) {
    await page.getByRole("button", { name: /create store/i }).click();
    const createDlg = page.getByRole("dialog", { name: /create store/i });
    await createDlg.getByLabel("Name").fill(name);
    await createDlg
      .getByLabel(/data directory/i)
      .fill(`${backendWithAdmin.dataRoot}/${name}`);
    await createDlg.getByRole("button", { name: /^create$/i }).click();
    await expect(createDlg).toBeHidden();
  }

  // Create an operator with a grant to `granted` only.
  await page.goto(`${backendWithAdmin.baseUrl}/users`);
  await page.getByRole("button", { name: /create user/i }).click();
  const createDlg = page.getByRole("dialog", { name: /create user/i });
  await createDlg.getByLabel("Username").fill("opbob");
  await createDlg.getByLabel(/^password$/i).fill("op-bob-initial-pw");
  await createDlg.getByLabel(/confirm password/i).fill("op-bob-initial-pw");
  // Role select stays at "operator" by default.
  // Toggle the `granted` chip. The underlying checkbox is sr-only,
  // so clicking the visible label is the right gesture.
  await createDlg.locator("label").filter({ hasText: /^granted$/ }).click();
  await createDlg.getByRole("button", { name: /^create$/i }).click();
  await expect(createDlg).toBeHidden();
  await expect(
    page.getByRole("cell", { name: "opbob", exact: true }),
  ).toBeVisible();

  // Admin logs out.
  await page.goto(`${backendWithAdmin.baseUrl}/logout`);
  await page.waitForURL(/\/login(\?|$)/);

  // Operator logs in.
  await loginViaUi(page, backendWithAdmin.baseUrl, "opbob", "op-bob-initial-pw");
  // Whoami reflects the operator role.
  const whoami = await page.request.get(
    `${backendWithAdmin.baseUrl}/v1/admin/auth/whoami`,
  );
  expect(await whoami.json()).toMatchObject({
    authenticated: true,
    username: "opbob",
    role: "operator",
  });

  // The Stores / Users nav items are hidden for non-admin sessions.
  // Both desktop sidebars and the mobile drawer share the same NAV
  // filter, so a presence check on the visible sidebar is enough.
  const navText = (await page.locator("aside").innerText()).toLowerCase();
  expect(navText).not.toContain("stores");
  expect(navText).not.toContain("users");
  expect(navText).toContain("profile");

  // Direct navigation to /users is redirected by the router.
  await page.goto(`${backendWithAdmin.baseUrl}/users`);
  await page.waitForURL(`${backendWithAdmin.baseUrl}/`);

  // The admin API enforces it too: GET /v1/admin/users → 403 even if
  // the SPA were tricked into calling it.
  const usersRes = await page.request.get(
    `${backendWithAdmin.baseUrl}/v1/admin/users`,
  );
  expect(usersRes.status()).toBe(403);

  // Cross-store probe: GET /v1/admin/stores/secret → 404 (denied_grant).
  const secretRes = await page.request.get(
    `${backendWithAdmin.baseUrl}/v1/admin/stores/secret`,
  );
  expect(secretRes.status()).toBe(404);

  // Change own password via /profile.
  await page.goto(`${backendWithAdmin.baseUrl}/profile`);
  await page.getByLabel(/current password/i).fill("op-bob-initial-pw");
  await page.getByLabel(/^new password$/i).fill("op-bob-rotated-pw");
  await page.getByLabel(/confirm new password/i).fill("op-bob-rotated-pw");
  await page.getByRole("button", { name: /^save$/i }).click();
  await expect(page.getByRole("status")).toContainText(/password updated/i);

  // Sign out, sign back in with the new password.
  await page.goto(`${backendWithAdmin.baseUrl}/logout`);
  await page.waitForURL(/\/login(\?|$)/);
  // Old password no longer works.
  await page.getByLabel("Username").fill("opbob");
  await page.getByLabel("Password").fill("op-bob-initial-pw");
  await page.getByRole("button", { name: /sign in/i }).click();
  await expect(
    page.getByText(/invalid username or password/i),
  ).toBeVisible();
  // New password works.
  await page.getByLabel("Password").fill("op-bob-rotated-pw");
  await page.getByRole("button", { name: /sign in/i }).click();
  await page.waitForURL(`${backendWithAdmin.baseUrl}/`);
});

test("last-admin protection rejects deleting the bootstrap admin via the API", async ({
  page,
  backendWithAdmin,
}) => {
  await loginViaUi(page, backendWithAdmin.baseUrl);
  // Find the bootstrap admin's id via the users list.
  const list = await page.request.get(
    `${backendWithAdmin.baseUrl}/v1/admin/users`,
  );
  const body = (await list.json()) as {
    users: Array<{ id: string; username: string; role: string }>;
  };
  const admin = body.users.find((u) => u.username === ADMIN_USERNAME);
  expect(admin).toBeDefined();
  // The CSRF cookie is set by login; replay it on the DELETE.
  const cookies = await page.context().cookies(backendWithAdmin.baseUrl);
  const csrf = cookies.find((c) => c.name === "__Host-uw-csrf");
  expect(csrf).toBeDefined();
  const del = await page.request.delete(
    `${backendWithAdmin.baseUrl}/v1/admin/users/${admin!.id}`,
    {
      headers: {
        "X-CSRF-Token": csrf!.value,
        Origin: backendWithAdmin.baseUrl,
      },
    },
  );
  expect(del.status(), `unexpected status; body: ${await del.text()}`).toBe(
    409,
  );
  const errBody = (await del.json()) as { code: string };
  expect(errBody.code).toBe("last_admin_protected");

  // And in fact the admin can still log in afterwards.
  await page.goto(`${backendWithAdmin.baseUrl}/logout`);
  await page.waitForURL(/\/login(\?|$)/);
  await page.getByLabel("Username").fill(ADMIN_USERNAME);
  await page.getByLabel("Password").fill(ADMIN_PASSWORD);
  await page.getByRole("button", { name: /sign in/i }).click();
  await page.waitForURL(`${backendWithAdmin.baseUrl}/`);
});

/**
 * User-management e2e.
 *
 * The product no longer has operator roles or per-store grants — every
 * user has full access (see the Users page note). What remains worth
 * covering end-to-end against a real backend:
 *
 *   1. An admin-created user can sign in and change their own password
 *      from /profile, and the rotation takes effect.
 *   2. Last-user deletion protection: the API refuses to delete the
 *      only remaining user.
 */
import {
  test,
  expect,
  loginViaUi,
  adminMutate,
  ADMIN_USERNAME,
  ADMIN_PASSWORD,
} from "./fixtures";

test("a created user can sign in and change their own password", async ({
  page,
  backendWithAdmin,
}) => {
  await loginViaUi(page, backendWithAdmin.baseUrl);

  // Create a second user.
  await page.goto(`${backendWithAdmin.baseUrl}/users`);
  await page.getByRole("button", { name: /create user/i }).click();
  const createDlg = page.getByRole("dialog", { name: /create user/i });
  await createDlg.getByLabel("Username").fill("opbob");
  await createDlg.getByLabel(/^password$/i).fill("op-bob-initial-pw");
  await createDlg.getByLabel(/confirm password/i).fill("op-bob-initial-pw");
  await createDlg.getByRole("button", { name: /^create$/i }).click();
  await expect(createDlg).toBeHidden();
  await expect(
    page.getByRole("cell", { name: "opbob", exact: true }),
  ).toBeVisible();

  // Admin logs out; the new user logs in.
  await page.goto(`${backendWithAdmin.baseUrl}/logout`);
  await page.waitForURL(/\/login(\?|$)/);
  await loginViaUi(page, backendWithAdmin.baseUrl, "opbob", "op-bob-initial-pw");
  const whoami = await page.request.get(
    `${backendWithAdmin.baseUrl}/v1/admin/auth/whoami`,
  );
  expect(await whoami.json()).toMatchObject({ username: "opbob" });

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
  await page.waitForURL(/\/secrets(\?|$)/);
});

test("last-user protection rejects deleting the only remaining user via the API", async ({
  page,
  backendWithAdmin,
}) => {
  await loginViaUi(page, backendWithAdmin.baseUrl);
  // Find the bootstrap user's id.
  const list = await page.request.get(
    `${backendWithAdmin.baseUrl}/v1/admin/users`,
  );
  const body = (await list.json()) as {
    users: Array<{ id: string; username: string }>;
  };
  const onlyUser = body.users.find((u) => u.username === ADMIN_USERNAME);
  expect(onlyUser).toBeDefined();

  // DELETE through the SPA's CSRF path (synchronizer token replay).
  const del = await adminMutate(
    page,
    "DELETE",
    `/v1/admin/users/${onlyUser!.id}`,
  );
  expect(del.status, `unexpected status; body: ${del.body}`).toBe(409);
  expect((JSON.parse(del.body) as { code: string }).code).toBe(
    "last_user_protected",
  );

  // The user can still log in afterwards.
  await page.goto(`${backendWithAdmin.baseUrl}/logout`);
  await page.waitForURL(/\/login(\?|$)/);
  await page.getByLabel("Username").fill(ADMIN_USERNAME);
  await page.getByLabel("Password").fill(ADMIN_PASSWORD);
  await page.getByRole("button", { name: /sign in/i }).click();
  await page.waitForURL(/\/secrets(\?|$)/);
});

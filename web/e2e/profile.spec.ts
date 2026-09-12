/**
 * Profile page e2e for the bootstrap operator.
 *
 * The first test verifies current-context refresh and lazy admin-API invalidation in a second browser context.
 * It retains the password roundtrip coverage against the real backend.
 */
import {
  test,
  expect,
  loginViaUi,
  ADMIN_USERNAME,
  ADMIN_PASSWORD,
} from "./fixtures";

test("bootstrap operator password change refreshes the current context, lazily invalidates a second context, and retains the password roundtrip", async ({
  page,
  browser,
  backendWithAdmin,
}) => {
  await loginViaUi(page, backendWithAdmin.baseUrl);
  const otherContext = await browser.newContext({ ignoreHTTPSErrors: true });

  try {
    const otherPage = await otherContext.newPage();
    await loginViaUi(otherPage, backendWithAdmin.baseUrl);

    const beforeWhoami = await otherContext.request.get(
      `${backendWithAdmin.baseUrl}/v1/admin/auth/whoami`,
    );
    expect(beforeWhoami.status()).toBe(200);
    expect(await beforeWhoami.json()).toEqual({ username: ADMIN_USERNAME });

    await otherPage.goto("about:blank");
    await expect(otherPage).toHaveURL("about:blank");

    await page.goto(`${backendWithAdmin.baseUrl}/profile`);

    // Assert the profile heading and operator identity without coupling to the surrounding copy.
    await expect(page.getByRole("heading", { name: /profile/i })).toBeVisible();
    await expect(page.getByRole("main")).toContainText(ADMIN_USERNAME);

    const guidance =
      "Underwrap attempts to refresh this session after the change so it can stay signed in. Other sessions issued before the change are invalidated when they next access the admin API. If this session cannot be refreshed, sign in again with the new password.";
    await expect(page.getByText(guidance, { exact: true })).toHaveText(
      guidance,
    );

    // Change password through the self-service form.
    const newPassword = `${ADMIN_PASSWORD}-rotated`;
    await page.getByLabel(/current password/i).fill(ADMIN_PASSWORD);
    await page.getByLabel(/^new password$/i).fill(newPassword);
    await page.getByLabel(/confirm new password/i).fill(newPassword);
    await page.getByRole("button", { name: /^save$/i }).click();
    await expect(page.getByRole("main").getByRole("status")).toContainText(
      /password updated/i,
    );

    const submittingWhoami = await page
      .context()
      .request.get(`${backendWithAdmin.baseUrl}/v1/admin/auth/whoami`);
    expect(submittingWhoami.status()).toBe(200);
    expect(await submittingWhoami.json()).toEqual({
      username: ADMIN_USERNAME,
    });

    const otherWhoami = await otherContext.request.get(
      `${backendWithAdmin.baseUrl}/v1/admin/auth/whoami`,
    );
    expect(otherWhoami.status()).toBe(200);
    expect(await otherWhoami.json()).toEqual({});

    await otherPage.goto(`${backendWithAdmin.baseUrl}/profile`);
    await otherPage.waitForURL(/\/login(\?|$)/);
    await expect(otherPage).toHaveURL(/\/login(\?|$)/);

    // Sign out and verify the round trip: old password fails, new works.
    await page.goto(`${backendWithAdmin.baseUrl}/logout`);
    await page.waitForURL(/\/login(\?|$)/);
    await page.getByLabel("Username").fill(ADMIN_USERNAME);
    await page.getByLabel("Password").fill(ADMIN_PASSWORD);
    await page.getByRole("button", { name: /sign in/i }).click();
    await expect(page.getByText(/invalid username or password/i)).toBeVisible();
    await page.getByLabel("Password").fill(newPassword);
    await page.getByRole("button", { name: /sign in/i }).click();
    await page.waitForURL(/\/setup(\?|$)/);
  } finally {
    await otherContext.close();
  }
});

test("profile form rejects a wrong current password without changing anything", async ({
  page,
  backendWithAdmin,
}) => {
  await loginViaUi(page, backendWithAdmin.baseUrl);
  await page.goto(`${backendWithAdmin.baseUrl}/profile`);

  await page
    .getByLabel(/current password/i)
    .fill("definitely-not-the-password");
  await page.getByLabel(/^new password$/i).fill("does-not-matter-1");
  await page.getByLabel(/confirm new password/i).fill("does-not-matter-1");
  await page.getByRole("button", { name: /^save$/i }).click();

  // An error banner surfaces; the success status does not appear.
  await expect(page.getByRole("alert")).toBeVisible();
  await expect(page.getByRole("main").getByRole("status")).toHaveCount(0);

  // The actual password is still the original — we can still sign in
  // with it after a forced logout.
  await page.goto(`${backendWithAdmin.baseUrl}/logout`);
  await page.waitForURL(/\/login(\?|$)/);
  await page.getByLabel("Username").fill(ADMIN_USERNAME);
  await page.getByLabel("Password").fill(ADMIN_PASSWORD);
  await page.getByRole("button", { name: /sign in/i }).click();
  await page.waitForURL(/\/setup(\?|$)/);
});

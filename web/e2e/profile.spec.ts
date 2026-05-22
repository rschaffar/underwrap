/**
 * Profile page e2e.
 *
 * The multi-user spec already exercises the operator-side flow
 * (operator logs in, visits /profile, changes their own password).
 * This spec covers the admin-side angle: the same self-service flow
 * for an admin user, plus the validation that the password-change
 * round trip works against the real backend.
 */
import { test, expect, loginViaUi, ADMIN_USERNAME, ADMIN_PASSWORD } from "./fixtures";

test("admin sees their identity on /profile and can change their password end to end", async ({
  page,
  backendWithAdmin,
}) => {
  await loginViaUi(page, backendWithAdmin.baseUrl);
  await page.goto(`${backendWithAdmin.baseUrl}/profile`);

  // Identity surfaces in the page header. The exact framing
  // ("Signed in as alice.") is what a reader actually sees, so
  // assert on substrings to stay resilient to copy changes.
  await expect(page.getByRole("heading", { name: /profile/i })).toBeVisible();
  await expect(page.getByRole("main")).toContainText(ADMIN_USERNAME);

  // Change password through the self-service form.
  const newPassword = `${ADMIN_PASSWORD}-rotated`;
  await page.getByLabel(/current password/i).fill(ADMIN_PASSWORD);
  await page.getByLabel(/^new password$/i).fill(newPassword);
  await page.getByLabel(/confirm new password/i).fill(newPassword);
  await page.getByRole("button", { name: /^save$/i }).click();
  await expect(page.getByRole("status")).toContainText(/password updated/i);

  // Sign out and verify the round trip: old password fails, new works.
  await page.goto(`${backendWithAdmin.baseUrl}/logout`);
  await page.waitForURL(/\/login(\?|$)/);
  await page.getByLabel("Username").fill(ADMIN_USERNAME);
  await page.getByLabel("Password").fill(ADMIN_PASSWORD);
  await page.getByRole("button", { name: /sign in/i }).click();
  await expect(
    page.getByText(/invalid username or password/i),
  ).toBeVisible();
  await page.getByLabel("Password").fill(newPassword);
  await page.getByRole("button", { name: /sign in/i }).click();
  await page.waitForURL(`${backendWithAdmin.baseUrl}/`);
});

test("profile form rejects a wrong current password without changing anything", async ({
  page,
  backendWithAdmin,
}) => {
  await loginViaUi(page, backendWithAdmin.baseUrl);
  await page.goto(`${backendWithAdmin.baseUrl}/profile`);

  await page.getByLabel(/current password/i).fill("definitely-not-the-password");
  await page.getByLabel(/^new password$/i).fill("does-not-matter-1");
  await page.getByLabel(/confirm new password/i).fill("does-not-matter-1");
  await page.getByRole("button", { name: /^save$/i }).click();

  // An error banner surfaces; the success status does not appear.
  await expect(page.getByRole("alert")).toBeVisible();
  await expect(page.getByRole("status")).toHaveCount(0);

  // The actual password is still the original — we can still sign in
  // with it after a forced logout.
  await page.goto(`${backendWithAdmin.baseUrl}/logout`);
  await page.waitForURL(/\/login(\?|$)/);
  await page.getByLabel("Username").fill(ADMIN_USERNAME);
  await page.getByLabel("Password").fill(ADMIN_PASSWORD);
  await page.getByRole("button", { name: /sign in/i }).click();
  await page.waitForURL(`${backendWithAdmin.baseUrl}/`);
});

/**
 * Dedicated Users CRUD e2e.
 *
 * `multi-user.spec.ts` already walks the create → operator-login →
 * password-change story end to end. This spec covers the rest of the
 * Users-page UI surface that the multi-user spec treats as setup:
 * edit, reset-password, delete, and the last-admin protection guard.
 *
 * Each test runs against a fresh backend with the bootstrap admin
 * already created (the `backendWithAdmin` fixture).
 */
import { test, expect, loginViaUi, ADMIN_USERNAME } from "./fixtures";
import type { Page } from "@playwright/test";

// Note: the working pattern shared with `multi-user.spec.ts` is to
// wait on a cell (one of the new row's text nodes), then walk to the
// enclosing row via `locator("xpath=..")`. A bare `getByRole("row")
// .filter({ hasText: /.../ })` does not pick up dynamically inserted
// rows reliably under React Query's refetch cadence.
async function rowFor(page: Page, username: string) {
  const cell = page.getByRole("cell", { name: username, exact: true });
  await expect(cell).toBeVisible();
  return cell.locator("xpath=ancestor::tr");
}

async function createUser(
  page: Page,
  username: string,
  password: string,
): Promise<void> {
  await page.getByRole("button", { name: /create user/i }).click();
  const dlg = page.getByRole("dialog", { name: /create user/i });
  await dlg.getByLabel("Username").fill(username);
  await dlg.getByLabel(/^password$/i).fill(password);
  await dlg.getByLabel(/confirm password/i).fill(password);
  await dlg.getByRole("button", { name: /^create$/i }).click();
  await expect(dlg).toBeHidden();
}

test("admin can rename a user", async ({ page, backendWithAdmin }) => {
  await loginViaUi(page, backendWithAdmin.baseUrl);
  await page.goto(`${backendWithAdmin.baseUrl}/users`);

  await createUser(page, "eve", "eve-initial-pw");
  const eveRow = await rowFor(page, "eve");

  // Rename eve → eve2. (There are no roles or per-store restrictions
  // any more; the edit dialog only renames.)
  await eveRow.getByRole("button", { name: /^edit$/i }).click();
  const editDlg = page.getByRole("dialog", { name: /edit user: eve/i });
  await editDlg.getByLabel("Username").fill("eve2");
  await editDlg.getByRole("button", { name: /^save$/i }).click();
  await expect(editDlg).toBeHidden();

  await rowFor(page, "eve2");
  // And the old name is gone.
  await expect(page.getByRole("cell", { name: "eve", exact: true })).toHaveCount(
    0,
  );
});

test("admin can reset an operator's password and the operator logs in with the new one", async ({
  page,
  backendWithAdmin,
}) => {
  await loginViaUi(page, backendWithAdmin.baseUrl);
  await page.goto(`${backendWithAdmin.baseUrl}/users`);

  await createUser(page, "carol", "carol-old-pw");
  const carolRow = await rowFor(page, "carol");

  // Reset carol's password.
  await carolRow.getByRole("button", { name: /reset password/i }).click();
  const resetDlg = page.getByRole("dialog", { name: /reset password: carol/i });
  await resetDlg.getByLabel(/^new password$/i).fill("carol-rotated-pw");
  await resetDlg.getByLabel(/confirm new password/i).fill("carol-rotated-pw");
  // The submit button is the second "Reset password" button on the
  // page — the first is the row action that opened the dialog. Scope
  // to the dialog to disambiguate.
  await resetDlg
    .getByRole("button", { name: /^reset password$/i })
    .click();
  await expect(resetDlg).toBeHidden();

  // Sign out the admin and verify carol's new password works (old one
  // does not). The reset path is what an admin uses to unblock an
  // operator who lost their password, so this round trip is the
  // value of the feature.
  await page.goto(`${backendWithAdmin.baseUrl}/logout`);
  await page.waitForURL(/\/login(\?|$)/);
  await page.getByLabel("Username").fill("carol");
  await page.getByLabel("Password").fill("carol-old-pw");
  await page.getByRole("button", { name: /sign in/i }).click();
  await expect(
    page.getByText(/invalid username or password/i),
  ).toBeVisible();
  await page.getByLabel("Password").fill("carol-rotated-pw");
  await page.getByRole("button", { name: /sign in/i }).click();
  await page.waitForURL(`${backendWithAdmin.baseUrl}/`);
});

test("admin can delete a non-last-admin operator", async ({
  page,
  backendWithAdmin,
}) => {
  await loginViaUi(page, backendWithAdmin.baseUrl);
  await page.goto(`${backendWithAdmin.baseUrl}/users`);

  await createUser(page, "dave", "dave-initial-pw");
  const daveRow = await rowFor(page, "dave");

  // Delete dave. The delete dialog uses a simple confirm/cancel —
  // no type-to-confirm gate.
  await daveRow.getByRole("button", { name: /^delete$/i }).click();
  const delDlg = page.getByRole("dialog", { name: /delete user: dave/i });
  const confirm = delDlg.getByRole("button", { name: /^delete$/i });
  await expect(confirm).toBeEnabled();
  await confirm.click();
  await expect(delDlg).toBeHidden();

  // Dave is gone from the list.
  await expect(page.getByRole("cell", { name: "dave", exact: true })).toHaveCount(
    0,
  );
});

test("delete dialog refuses to arm for the last remaining user", async ({
  page,
  backendWithAdmin,
}) => {
  await loginViaUi(page, backendWithAdmin.baseUrl);
  await page.goto(`${backendWithAdmin.baseUrl}/users`);

  // The bootstrap user is the only user on a fresh fixture. The current
  // session's row labels the cell as `alice(you)`, so match by
  // inclusion instead of exact.
  const userCell = page
    .getByRole("cell")
    .filter({ hasText: new RegExp(`\\b${ADMIN_USERNAME}\\b`) })
    .first();
  await expect(userCell).toBeVisible();
  const userRow = userCell.locator("xpath=ancestor::tr");

  await userRow.getByRole("button", { name: /^delete$/i }).click();
  const delDlg = page.getByRole("dialog", {
    name: new RegExp(`delete user: ${ADMIN_USERNAME}`, "i"),
  });

  // The warning banner appears.
  await expect(
    delDlg.getByRole("alert").filter({ hasText: /last remaining user/i }),
  ).toBeVisible();

  // The destructive button stays disabled.
  const confirm = delDlg.getByRole("button", { name: /^delete$/i });
  await expect(confirm).toBeDisabled();
});

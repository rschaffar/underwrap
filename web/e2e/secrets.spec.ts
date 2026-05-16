/**
 * Secrets CRUD e2e (ticket 300, Option A).
 *
 * Exercises the new /secrets page end-to-end against a real backend:
 *
 *   - Empty state surfaces a friendly hint and a disabled "Create
 *     secret" button (writes are off on a fresh store).
 *   - With writes enabled, the operator can create a secret, see it
 *     in the list, reveal it (and the dialog requires explicit
 *     confirmation), overwrite it, and delete it (with the
 *     type-to-confirm guard).
 *   - Reserved __underwrap/ IDs (policy, catalog, audit) are hidden.
 */
import { test, expect, loginViaUi } from "./fixtures";

test("secrets page disables create / edit / delete when writes_enabled is off", async ({
  page,
  backendWithAdmin,
}) => {
  await loginViaUi(page, backendWithAdmin.baseUrl);
  await page.goto(`${backendWithAdmin.baseUrl}/secrets`);

  await expect(page.getByRole("heading", { name: /^secrets$/i })).toBeVisible();
  // The "writes disabled" status banner is visible.
  await expect(page.getByRole("status")).toContainText(/writes are currently disabled/i);
  // The create-secret button is disabled.
  await expect(
    page.getByRole("button", { name: /create secret/i }),
  ).toBeDisabled();
});

test("secrets full round trip: create → reveal → edit → delete", async ({
  page,
  backendWithAdmin,
}) => {
  await loginViaUi(page, backendWithAdmin.baseUrl);

  // Flip writes on via the Matrix page so the Secrets dialogs unlock.
  await page.goto(`${backendWithAdmin.baseUrl}/matrix`);
  await page.getByTestId("writes-toggle-input").click();
  await expect(page.getByTestId("writes-toggle-input")).toBeChecked();

  await page.goto(`${backendWithAdmin.baseUrl}/secrets`);

  // Empty initial list — fresh store.
  await expect(
    page.getByText(/no secrets in this store yet/i),
  ).toBeVisible();

  // Create a secret.
  await page.getByRole("button", { name: /create secret/i }).click();
  const createDlg = page.getByRole("dialog", { name: /create secret/i });
  await createDlg.getByLabel("Secret ID").fill("github/token/env/GH_TOKEN");
  // Wrapper field is free-form (the catalog is empty on a fresh
  // store, so the dialog falls back to a plain input).
  await createDlg.getByLabel(/wrapper context/i).fill("gh-authorized");
  await createDlg.getByTestId("material-input").fill("ghp_secret_value_v1");
  await createDlg.getByRole("button", { name: /^create$/i }).click();
  await expect(createDlg).toBeHidden();

  // The new secret shows up in the table.
  await expect(
    page.getByRole("cell", { name: "github/token/env/GH_TOKEN" }),
  ).toBeVisible();

  // The reserved __underwrap/ policy document was just written by
  // the toggle flip — confirm it does not appear in the visible list.
  await expect(
    page.getByRole("cell", { name: /__underwrap\//i }),
  ).toHaveCount(0);

  // Reveal: the confirmation step gates the actual read.
  const row = page
    .getByRole("cell", { name: "github/token/env/GH_TOKEN" })
    .locator("xpath=ancestor::tr");
  await row.getByRole("button", { name: /reveal/i }).click();
  const revealDlg = page.getByRole("dialog", { name: /reveal secret/i });
  await expect(revealDlg).toBeVisible();
  // Value is NOT shown until the operator presses Show.
  await expect(revealDlg.getByTestId("reveal-value")).toHaveCount(0);
  await revealDlg.getByRole("button", { name: /^show$/i }).click();
  await expect(revealDlg.getByTestId("reveal-value")).toHaveValue(
    "ghp_secret_value_v1",
  );
  await revealDlg.getByText("Close", { exact: true }).click();
  await expect(revealDlg).toBeHidden();

  // Edit: overwrite the material. The secret_id stays read-only on
  // the edit path so it can't accidentally migrate.
  await row.getByRole("button", { name: /^edit$/i }).click();
  const editDlg = page.getByRole("dialog", {
    name: /edit: github\/token\/env\/GH_TOKEN/i,
  });
  await expect(editDlg.getByLabel("Secret ID")).toHaveAttribute("readonly", "");
  // Wrapper field isn't persisted with the secret (the backend's
  // secret list only returns IDs), so the edit dialog opens it blank
  // — refill it so the audit event still ties to a real wrapper.
  await editDlg.getByLabel(/wrapper context/i).fill("gh-authorized");
  await editDlg.getByTestId("material-input").fill("ghp_secret_value_v2");
  await editDlg.getByRole("button", { name: /^save$/i }).click();
  await expect(editDlg).toBeHidden();

  // Re-reveal to confirm the overwrite landed.
  await row.getByRole("button", { name: /reveal/i }).click();
  await page.getByRole("button", { name: /^show$/i }).click();
  await expect(page.getByTestId("reveal-value")).toHaveValue(
    "ghp_secret_value_v2",
  );
  await page.getByText("Close", { exact: true }).click();

  // Delete: type-to-confirm gates the destructive button.
  await row.getByRole("button", { name: /^delete$/i }).click();
  const delDlg = page.getByRole("dialog", { name: /delete secret/i });
  const delBtn = delDlg.getByRole("button", { name: /^delete$/i });
  await expect(delBtn).toBeDisabled();
  await delDlg.getByLabel(/type .* to confirm/i).fill("github/token/env/GH_TOKEN");
  await expect(delBtn).toBeEnabled();
  await delBtn.click();
  await expect(delDlg).toBeHidden();

  // The secret is gone from the list.
  await expect(
    page.getByRole("cell", { name: "github/token/env/GH_TOKEN" }),
  ).toHaveCount(0);
});

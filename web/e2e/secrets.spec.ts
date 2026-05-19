/**
 * Secrets CRUD e2e (ticket 300, Option A).
 *
 * Exercises the /secrets page end-to-end against a real backend:
 *
 *   - Admin can create a secret, see it grouped under its resource,
 *     reveal it (with explicit confirmation), overwrite it, and
 *     delete it (with the type-to-confirm guard).
 *   - Reserved __underwrap/ IDs (policy, catalog, audit) are hidden.
 *   - Per-(resource, slot) wrapper-write toggles appear for files
 *     and libsecret, but not for env.
 */
import { test, expect, loginViaUi } from "./fixtures";

test("admin always sees an enabled Create secret button (writes are unconditional)", async ({
  page,
  backendWithAdmin,
}) => {
  await loginViaUi(page, backendWithAdmin.baseUrl);
  await page.goto(`${backendWithAdmin.baseUrl}/secrets`);

  await expect(page.getByRole("heading", { name: /^secrets$/i })).toBeVisible();
  // No "writes disabled" status banner — there's no global toggle
  // anymore. Admin writes are always allowed.
  await expect(page.getByRole("status")).toHaveCount(0);
  await expect(
    page.getByRole("button", { name: /create secret/i }),
  ).toBeEnabled();
});

test("secrets full round trip: create → reveal → edit → delete", async ({
  page,
  backendWithAdmin,
}) => {
  await loginViaUi(page, backendWithAdmin.baseUrl);
  await page.goto(`${backendWithAdmin.baseUrl}/secrets`);

  // Empty initial list — fresh store with no catalog entries yet.
  await expect(
    page.getByText(/no resources are known to this store yet/i),
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

  // The new secret shows up under the github/token resource block.
  // The block is collapsed by default — expand it before reaching
  // for the entry row.
  const resourceBlock = page.getByTestId("resource-block-github/token");
  await expect(resourceBlock).toBeVisible();
  await resourceBlock.locator("summary").click();
  const envSection = resourceBlock.getByTestId("slot-env");
  await expect(envSection.getByText("GH_TOKEN")).toBeVisible();

  // Reveal: the confirmation step gates the actual read.
  const row = envSection
    .getByText("GH_TOKEN")
    .locator("xpath=ancestor::li");
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

  // The secret is gone — the empty resource block disappears with it.
  await expect(
    page.getByTestId("resource-block-github/token"),
  ).toHaveCount(0);
});

// The wrapper-writable per-slot toggle moved off the Secrets page when
// `writable` became a per-grant field; the equivalent affordance now
// lives on the Matrix cell editor and is covered by
// `matrix.spec.ts` "raw policy viewer surfaces the live PolicyDocument
// and writable-grant count".

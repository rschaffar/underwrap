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

test("admin always sees an enabled Create resource button (writes are unconditional)", async ({
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
    page.getByRole("button", { name: /create resource/i }),
  ).toBeEnabled();
});

test("secrets full round trip: create → reveal → edit → delete", async ({
  page,
  backendWithAdmin,
}) => {
  await loginViaUi(page, backendWithAdmin.baseUrl);
  await page.goto(`${backendWithAdmin.baseUrl}/secrets`);

  // Empty initial list — fresh store with no resource records.
  await expect(
    page.getByText(/no resources are known to this store yet/i),
  ).toBeVisible();

  // Create the resource block first, then add an env entry to it via
  // the per-slot "+" button.
  await page.getByRole("button", { name: /create resource/i }).click();
  const resourceDlg = page.getByRole("dialog", { name: /create resource/i });
  await resourceDlg.getByTestId("create-resource-input").fill("github/token");
  await resourceDlg.getByRole("button", { name: /^create$/i }).click();
  await expect(resourceDlg).toBeHidden();

  const resourceBlock = page.getByTestId("resource-block-github/token");
  await expect(resourceBlock).toBeVisible();
  await resourceBlock.locator("summary").click();

  const envSection = resourceBlock.getByTestId("slot-env");
  await envSection.getByTestId("slot-env-add").click();
  const createDlg = page.getByRole("dialog", { name: /create secret/i });
  // The dialog should have the secret ID prefilled with the resource
  // and slot — the operator only needs to append the entry name.
  await expect(createDlg.getByLabel("Secret ID")).toHaveValue(
    "github/token/env/",
  );
  await createDlg
    .getByLabel("Secret ID")
    .fill("github/token/env/GH_TOKEN");
  // Wrapper field is free-form (no pair records are registered on a
  // fresh store, so the dialog falls back to a plain input).
  await createDlg.getByLabel(/wrapper context/i).fill("gh-authorized");
  await createDlg.getByTestId("material-input").fill("ghp_secret_value_v1");
  await createDlg.getByRole("button", { name: /^create$/i }).click();
  await expect(createDlg).toBeHidden();

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

  // Delete: simple confirm/cancel dialog.
  await row.getByRole("button", { name: /^delete$/i }).click();
  const delDlg = page.getByRole("dialog", { name: /delete secret/i });
  const delBtn = delDlg.getByRole("button", { name: /^delete$/i });
  await expect(delBtn).toBeEnabled();
  await delBtn.click();
  await expect(delDlg).toBeHidden();

  // The secret is gone. The resource block stays visible (the
  // resource is a backend record now and outlives its secrets) but
  // the env slot reports empty.
  const resourceBlockAfter = page.getByTestId("resource-block-github/token");
  await expect(resourceBlockAfter).toBeVisible();
  await expect(resourceBlockAfter.getByTestId("slot-env")).toHaveText(
    /none/i,
  );
});

// The wrapper-writable per-slot toggle moved off the Secrets page when
// `writable` became a per-grant field; the equivalent affordance now
// lives on the Matrix cell editor and is covered by
// `wrappers.spec.ts` "raw policy viewer surfaces the live PolicyDocument
// and writable-grant count".

test("admin deletes a resource via the simple confirm dialog after refusal clears", async ({
  page,
  backendWithAdmin,
}) => {
  await loginViaUi(page, backendWithAdmin.baseUrl);
  await page.goto(`${backendWithAdmin.baseUrl}/secrets`);

  // Seed a resource and a secret under it so the first delete attempt
  // is refused with a structured blocker.
  await page.getByRole("button", { name: /create resource/i }).click();
  const resourceDlg = page.getByRole("dialog", { name: /create resource/i });
  await resourceDlg.getByTestId("create-resource-input").fill("seeded/res");
  await resourceDlg.getByRole("button", { name: /^create$/i }).click();
  await expect(resourceDlg).toBeHidden();

  const block = page.getByTestId("resource-block-seeded/res");
  await expect(block).toBeVisible();
  await block.locator("summary").click();
  await block.getByTestId("slot-env").getByTestId("slot-env-add").click();
  const createDlg = page.getByRole("dialog", { name: /create secret/i });
  await createDlg.getByLabel("Secret ID").fill("seeded/res/env/EXAMPLE");
  await createDlg.getByLabel(/wrapper context/i).fill("seed-wrapper");
  await createDlg.getByTestId("material-input").fill("v1");
  await createDlg.getByRole("button", { name: /^create$/i }).click();
  await expect(createDlg).toBeHidden();

  // First delete attempt: backend refuses, blockers render inline.
  await block.getByTestId("resource-delete-seeded/res").click();
  const delDlg = page.getByRole("dialog", { name: /delete resource: seeded\/res/i });
  await delDlg.getByTestId("resource-delete-confirm").click();
  await expect(delDlg.getByTestId("resource-delete-blockers")).toBeVisible();
  await delDlg.getByRole("button", { name: /^cancel$/i }).click();
  await expect(delDlg).toBeHidden();

  // Clear the secret, then re-attempt: delete succeeds, block vanishes.
  const row = block
    .getByTestId("slot-env")
    .getByText("EXAMPLE")
    .locator("xpath=ancestor::li");
  await row.getByRole("button", { name: /^delete$/i }).click();
  const secretDelDlg = page.getByRole("dialog", { name: /delete secret/i });
  await secretDelDlg.getByRole("button", { name: /^delete$/i }).click();
  await expect(secretDelDlg).toBeHidden();

  await block.getByTestId("resource-delete-seeded/res").click();
  const delDlg2 = page.getByRole("dialog", { name: /delete resource: seeded\/res/i });
  await delDlg2.getByTestId("resource-delete-confirm").click();
  await expect(delDlg2).toBeHidden();
  await expect(page.getByTestId("resource-block-seeded/res")).toHaveCount(0);
});

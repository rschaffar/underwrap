/**
 * Secrets CRUD e2e.
 *
 * Exercises the /secrets page end-to-end against a real backend:
 *
 *   - Admin can create a resource and per-slot entries (env, libsecret),
 *     reveal them (with explicit confirmation), overwrite them, and
 *     delete them via the simple confirm dialog.
 *   - Resource deletion is refused while dependents exist and succeeds
 *     once cleared.
 *
 * Resource blocks are keyed by stable backend id (`res_xxx`), not by
 * name, so these tests locate blocks by their visible name through
 * `hasText` filters and use the name-based aria-labels for action
 * buttons.
 */
import type { Locator, Page } from "@playwright/test";
import { test, expect, loginViaUi } from "./fixtures";

/**
 * The `data-testid="resource-block-..."` attribute is keyed by the
 * stable backend id, which the operator never sees. Locate by visible
 * name instead — that's also how a real user finds the block.
 */
function resourceBlockByName(page: Page, name: string): Locator {
  return page
    .locator('[data-testid^="resource-block-"]')
    .filter({ hasText: name });
}

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

test("secrets full round trip: create → reveal → edit → delete (env slot)", async ({
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

  const resourceBlock = resourceBlockByName(page, "github/token");
  await expect(resourceBlock).toBeVisible();
  await resourceBlock.locator("summary").click();

  const envSection = resourceBlock.getByTestId("slot-env");
  await envSection.getByTestId("slot-env-add").click();
  const createDlg = page.getByRole("dialog", { name: /create env entry/i });
  await createDlg.getByLabel("Key").fill("GH_TOKEN");
  await createDlg.getByTestId("material-input").fill("ghp_secret_value_v1");
  await createDlg.getByRole("button", { name: /^create$/i }).click();
  await expect(createDlg).toBeHidden();

  await expect(envSection.getByText("GH_TOKEN")).toBeVisible();

  // Reveal: the confirmation step gates the actual read.
  const row = envSection.getByText("GH_TOKEN").locator("xpath=ancestor::li");
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

  // Edit: overwrite the material. The current dialog title is
  // "Edit <slot> entry"; the key field is editable but we keep it
  // stable for this assertion.
  await row.getByRole("button", { name: /^edit$/i }).click();
  const editDlg = page.getByRole("dialog", { name: /edit env entry/i });
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
  const resourceBlockAfter = resourceBlockByName(page, "github/token");
  await expect(resourceBlockAfter).toBeVisible();
  await expect(resourceBlockAfter.getByTestId("slot-env")).toHaveText(/none/i);
});

test("admin creates and deletes a libsecret slot entry", async ({
  page,
  backendWithAdmin,
}) => {
  // Mirror the env round-trip for the libsecret slot. The Delete
  // dialog and `/v1/admin/secret/delete` handler are slot-agnostic on
  // both client and server, but libsecret keys round-trip through a
  // richer canonical form (aliases + collection meta + item
  // meta+secret) — exercising a real key end-to-end is the only way
  // to catch a regression in that path.
  await loginViaUi(page, backendWithAdmin.baseUrl);
  await page.goto(`${backendWithAdmin.baseUrl}/secrets`);

  await page.getByRole("button", { name: /create resource/i }).click();
  const resourceDlg = page.getByRole("dialog", { name: /create resource/i });
  await resourceDlg.getByTestId("create-resource-input").fill("libsecret/demo");
  await resourceDlg.getByRole("button", { name: /^create$/i }).click();
  await expect(resourceDlg).toBeHidden();

  const block = resourceBlockByName(page, "libsecret/demo");
  await expect(block).toBeVisible();
  await block.locator("summary").click();

  const libsecretSection = block.getByTestId("slot-libsecret");
  await libsecretSection.getByTestId("slot-libsecret-add").click();
  const createDlg = page.getByRole("dialog", { name: /create libsecret entry/i });
  // `_aliases` survives the canonical round-trip when its JSON payload
  // is non-empty: `libsecret_section_to_kv` only re-emits `_aliases`
  // when aliases or collections are non-empty (see
  // `shared-store/src/resource_blob.rs`).
  await createDlg.getByLabel("Key").fill("_aliases");
  await createDlg.getByTestId("material-input").fill('{"default":"abc"}');
  await createDlg.getByRole("button", { name: /^create$/i }).click();
  await expect(createDlg).toBeHidden();

  await expect(libsecretSection.getByText("_aliases")).toBeVisible();

  // Delete the single libsecret entry. The resource record stays.
  const row = libsecretSection
    .getByText("_aliases")
    .locator("xpath=ancestor::li");
  await row.getByRole("button", { name: /^delete$/i }).click();
  const delDlg = page.getByRole("dialog", { name: /delete secret/i });
  await delDlg.getByRole("button", { name: /^delete$/i }).click();
  await expect(delDlg).toBeHidden();

  const blockAfter = resourceBlockByName(page, "libsecret/demo");
  await expect(blockAfter).toBeVisible();
  await expect(blockAfter.getByTestId("slot-libsecret")).toHaveText(/none/i);
});

// The wrapper-writable per-slot toggle moved off the Secrets page
// when `writable` became a per-permission field; the equivalent
// affordance now lives on the Wrapper permissions cell editor and is
// covered by `wrappers.spec.ts` "raw policy viewer surfaces the live
// PolicyDocument and writable-permission count".

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

  const block = resourceBlockByName(page, "seeded/res");
  await expect(block).toBeVisible();
  await block.locator("summary").click();
  await block.getByTestId("slot-env").getByTestId("slot-env-add").click();
  const createDlg = page.getByRole("dialog", { name: /create env entry/i });
  await createDlg.getByLabel("Key").fill("EXAMPLE");
  await createDlg.getByTestId("material-input").fill("v1");
  await createDlg.getByRole("button", { name: /^create$/i }).click();
  await expect(createDlg).toBeHidden();

  // First delete attempt: backend refuses, blockers render inline.
  // The resource-delete button has aria-label "Delete resource <name>".
  await block
    .getByRole("button", { name: /^delete resource seeded\/res$/i })
    .click();
  const delDlg = page.getByRole("dialog", {
    name: /delete resource: seeded\/res/i,
  });
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

  await block
    .getByRole("button", { name: /^delete resource seeded\/res$/i })
    .click();
  const delDlg2 = page.getByRole("dialog", {
    name: /delete resource: seeded\/res/i,
  });
  await delDlg2.getByTestId("resource-delete-confirm").click();
  await expect(delDlg2).toBeHidden();
  await expect(resourceBlockByName(page, "seeded/res")).toHaveCount(0);
});

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

test("admin builds a libsecret tree end-to-end (collection + item + alias)", async ({
  page,
  backendWithAdmin,
}) => {
  // Exercises the structured libsecret admin surface end-to-end:
  // create a collection, add an item with a secret + attributes, set
  // an alias targeting that collection, reveal the item secret, then
  // tear it all down via the alias-, item-, and collection-delete
  // verbs. Each verb maps 1:1 to a Secret Service spec concept; none
  // of them go through the flat-KV `_aliases`/`_meta`/`.secret`
  // bridge.
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

  const tree = block.getByTestId("libsecret-tree");
  await expect(tree).toBeVisible();

  // Create a collection with explicit id `default`.
  await tree.getByTestId("libsecret-add-collection").click();
  const colDlg = page.getByRole("dialog", {
    name: /create libsecret collection/i,
  });
  await colDlg.getByLabel(/^id/i).fill("default");
  await colDlg.getByLabel(/^label/i).fill("Default");
  await colDlg.getByRole("button", { name: /^create$/i }).click();
  await expect(colDlg).toBeHidden();

  const collection = tree.getByTestId("libsecret-collection-default");
  await expect(collection).toBeVisible();
  await collection.locator("summary").click();

  // Create an item inside that collection.
  await collection.getByTestId("libsecret-add-item-default").click();
  const itemDlg = page.getByRole("dialog", {
    name: /create libsecret item/i,
  });
  await itemDlg.getByLabel(/^label/i).fill("GitHub CLI");
  await itemDlg.getByLabel(/^content type/i).fill("text/plain");
  await itemDlg
    .getByTestId("libsecret-item-attributes-input")
    .fill("xdg:schema=org.freedesktop.Secret.Generic\nservice=github.com");
  await itemDlg
    .getByTestId("libsecret-item-secret-input")
    .fill("ghp_e2e_value_v1");
  await itemDlg.getByRole("button", { name: /^create$/i }).click();
  await expect(itemDlg).toBeHidden();

  const item = tree.getByTestId("libsecret-item-default-item_1");
  await expect(item).toBeVisible();
  await expect(item).toHaveText(/GitHub CLI/);
  await expect(item).toHaveText(/text\/plain/);
  await expect(item).toHaveText(/xdg:schema=org.freedesktop.Secret.Generic/);

  // Reveal the item secret. The Show step gates the read; the value
  // surfaces only after confirmation.
  await item.getByRole("button", { name: /reveal/i }).click();
  const revealDlg = page.getByRole("dialog", {
    name: /reveal libsecret item secret/i,
  });
  await expect(revealDlg).toBeVisible();
  await expect(revealDlg.getByTestId("libsecret-reveal-value")).toHaveCount(0);
  await revealDlg.getByRole("button", { name: /^show$/i }).click();
  await expect(revealDlg.getByTestId("libsecret-reveal-value")).toHaveValue(
    "ghp_e2e_value_v1",
  );
  await revealDlg.getByText("Close", { exact: true }).click();
  await expect(revealDlg).toBeHidden();

  // Set the `default` alias pointing at the collection.
  const aliases = tree.getByTestId("libsecret-aliases");
  await aliases.getByTestId("libsecret-set-alias").click();
  const aliasDlg = page.getByRole("dialog", {
    name: /set libsecret alias/i,
  });
  await aliasDlg.getByLabel(/alias name/i).fill("default");
  await aliasDlg.getByRole("button", { name: /^save$/i }).click();
  await expect(aliasDlg).toBeHidden();

  const aliasRow = tree.getByTestId("libsecret-alias-default");
  await expect(aliasRow).toContainText(/default\s*→\s*default/);
  await expect(aliasRow).not.toContainText(/dangling/i);

  // Delete the alias. Collection + item stay.
  await aliasRow.getByRole("button", { name: /^delete$/i }).click();
  const delAliasDlg = page.getByRole("dialog", { name: /delete alias/i });
  await delAliasDlg.getByRole("button", { name: /^delete$/i }).click();
  await expect(delAliasDlg).toBeHidden();
  await expect(aliases).not.toHaveText(/libsecret-alias-default/);

  // Delete the item.
  await item.getByRole("button", { name: /^delete$/i }).click();
  const delItemDlg = page.getByRole("dialog", { name: /delete item/i });
  await delItemDlg.getByRole("button", { name: /^delete$/i }).click();
  await expect(delItemDlg).toBeHidden();
  await expect(
    tree.getByTestId("libsecret-item-default-item_1"),
  ).toHaveCount(0);

  // Finally, delete the collection itself.
  await collection
    .getByTestId("libsecret-delete-collection-default")
    .click();
  const delColDlg = page.getByRole("dialog", { name: /delete collection/i });
  await delColDlg.getByRole("button", { name: /^delete$/i }).click();
  await expect(delColDlg).toBeHidden();

  // The libsecret tree is empty again. The resource record stays.
  const blockAfter = resourceBlockByName(page, "libsecret/demo");
  await expect(blockAfter).toBeVisible();
  await expect(blockAfter.getByTestId("libsecret-tree")).toHaveText(/none/i);
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

  // Seed a resource with both baseline env content and structured libsecret
  // content so deletion reports both lifecycle blockers.
  await page.getByRole("button", { name: /create resource/i }).click();
  const resourceDlg = page.getByRole("dialog", { name: /create resource/i });
  await resourceDlg.getByTestId("create-resource-input").fill("seeded/res");
  await resourceDlg.getByRole("button", { name: /^create$/i }).click();
  await expect(resourceDlg).toBeHidden();

  const block = resourceBlockByName(page, "seeded/res");
  await expect(block).toBeVisible();
  await block.locator("summary").click();
  await block.getByTestId("slot-env").getByTestId("slot-env-add").click();
  const createEnvDlg = page.getByRole("dialog", { name: /create env entry/i });
  await createEnvDlg.getByLabel("Key").fill("EXAMPLE");
  await createEnvDlg.getByTestId("material-input").fill("v1");
  await createEnvDlg.getByRole("button", { name: /^create$/i }).click();
  await expect(createEnvDlg).toBeHidden();

  const tree = block.getByTestId("libsecret-tree");
  await tree.getByTestId("libsecret-add-collection").click();
  const createCollectionDlg = page.getByRole("dialog", {
    name: /create libsecret collection/i,
  });
  await createCollectionDlg.getByLabel(/^id/i).fill("delete-blocker");
  await createCollectionDlg.getByLabel(/^label/i).fill("Delete blocker");
  await createCollectionDlg.getByRole("button", { name: /^create$/i }).click();
  await expect(createCollectionDlg).toBeHidden();

  // First delete attempt: both blocker kinds render inline.
  await block
    .getByRole("button", { name: /^delete resource seeded\/res$/i })
    .click();
  const delDlg = page.getByRole("dialog", {
    name: /delete resource: seeded\/res/i,
  });
  await delDlg.getByTestId("resource-delete-confirm").click();
  const blockers = delDlg.getByTestId("resource-delete-blockers");
  await expect(blockers).toBeVisible();
  await expect(blockers).toContainText("env: EXAMPLE");
  await expect(blockers).toContainText(
    "libsecret collection: delete-blocker",
  );
  await delDlg.getByRole("button", { name: /^cancel$/i }).click();
  await expect(delDlg).toBeHidden();

  // Preserve the baseline env lifecycle: clear it through the UI.
  const envRow = block
    .getByTestId("slot-env")
    .getByText("EXAMPLE")
    .locator("xpath=ancestor::li");
  await envRow.getByRole("button", { name: /^delete$/i }).click();
  const envDeleteDlg = page.getByRole("dialog", { name: /delete secret/i });
  await envDeleteDlg.getByRole("button", { name: /^delete$/i }).click();
  await expect(envDeleteDlg).toBeHidden();

  // A second attempt leaves the resource dialog open and reports only the
  // remaining libsecret blocker.
  await block
    .getByRole("button", { name: /^delete resource seeded\/res$/i })
    .click();
  await delDlg.getByTestId("resource-delete-confirm").click();
  await expect(delDlg).toBeVisible();
  await expect(blockers).toBeVisible();
  await expect(blockers).not.toContainText("env: EXAMPLE");
  await expect(blockers).toContainText(
    "libsecret collection: delete-blocker",
  );
  await delDlg.getByRole("button", { name: /^cancel$/i }).click();
  await expect(delDlg).toBeHidden();

  await tree
    .getByTestId("libsecret-delete-collection-delete-blocker")
    .click();
  const contentDeleteDlg = page.getByRole("dialog", {
    name: /delete collection/i,
  });
  await contentDeleteDlg.getByRole("button", { name: /^delete$/i }).click();
  await expect(contentDeleteDlg).toBeHidden();

  await block
    .getByRole("button", { name: /^delete resource seeded\/res$/i })
    .click();
  await delDlg.getByTestId("resource-delete-confirm").click();
  await expect(delDlg).toBeHidden();
  await expect(resourceBlockByName(page, "seeded/res")).toHaveCount(0);
});

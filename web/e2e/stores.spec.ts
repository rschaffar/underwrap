/**
 * Stores page e2e.
 *
 * Reuses the per-test `backendWithAdmin` fixture so the page renders
 * against a real backend. The fixture's default store is `default`
 * (dynamic), so the create/edit/delete paths can hit a fresh tree
 * without colliding with anything pre-existing.
 *
 * The fixture's backend has `stores_data_root` set to a per-test temp
 * directory, so the create-store flow drops new store data dirs under
 * `<dataRoot>/<store_id>/` without any operator-chosen path.
 */
import type { Locator, Page, Request } from "@playwright/test";

import { test, expect, loginViaUi } from "./fixtures";

function storeRow(page: Page, name: string): Locator {
  return page.getByRole("row", { name: new RegExp(`\\b${name}\\b`) });
}

async function displayedGeneratedKey(page: Page): Promise<string> {
  const key = (await page.getByTestId("generated-store-key").innerText()).trim();
  expect(
    /^[0-9a-f]{64}$/.test(key),
    "the backend-generated key should have canonical shape",
  ).toBe(true);
  return key;
}

async function completeExactKeyConfirmation(
  page: Page,
  key: string,
  action: RegExp,
): Promise<void> {
  await page.getByTestId("store-key-reentry").fill(key);
  await page.getByRole("button", { name: action }).click();
  await expect(page.getByTestId("key-confirmation-dialog")).toBeHidden();
}

async function createEncryptedStore(
  page: Page,
  name: string,
  description = "",
): Promise<string> {
  await page.getByRole("button", { name: /create store/i }).click();
  const dialog = page.getByRole("dialog", { name: /create store/i });
  await dialog.getByLabel("Name").fill(name);
  if (description) {
    await dialog.getByLabel("Description (optional)").fill(description);
  }
  await dialog.getByLabel(/backend/i).selectOption("encrypted");
  await dialog
    .getByRole("button", { name: /generate and review key/i })
    .click();
  const key = await displayedGeneratedKey(page);
  await completeExactKeyConfirmation(page, key, /^create encrypted store$/i);
  await expect(storeRow(page, name)).toBeVisible();
  return key;
}

async function beginPlainToEncryptedMigration(
  page: Page,
  storeName: string,
): Promise<string> {
  await storeRow(page, storeName)
    .getByRole("button", { name: /migrate to encrypted/i })
    .click();
  const dialog = page.getByRole("dialog", {
    name: new RegExp(`migrate to encrypted: ${storeName}`, "i"),
  });
  await expect(dialog).toContainText(/quiesce wrapper traffic/i);
  await expect(dialog).toContainText(
    /plaintext source data under a private backup/i,
  );
  await dialog
    .getByRole("button", { name: /generate and review key/i })
    .click();
  return displayedGeneratedKey(page);
}

async function checkedBrowserStateContainsKey(
  page: Page,
  key: string,
): Promise<boolean> {
  const browserContains = await page.evaluate(async (sensitive) => {
    const storageContains = (storage: Storage) =>
      Object.entries(storage).some(([name, value]) =>
        `${name}\u0000${value}`.includes(sensitive),
      );
    const cacheNames = "caches" in globalThis ? await caches.keys() : [];
    let cacheContains = false;
    for (const cacheName of cacheNames) {
      const cache = await caches.open(cacheName);
      const requests = await cache.keys();
      if (requests.some((request) => request.url.includes(sensitive))) {
        cacheContains = true;
      }
      for (const request of requests) {
        const response = await cache.match(request);
        if (response && (await response.clone().text()).includes(sensitive)) {
          cacheContains = true;
        }
      }
    }
    const databaseNames =
      "indexedDB" in globalThis && "databases" in indexedDB
        ? (await indexedDB.databases()).map((database) => database.name ?? "")
        : [];
    return (
      storageContains(localStorage) ||
      storageContains(sessionStorage) ||
      document.cookie.includes(sensitive) ||
      window.location.href.includes(sensitive) ||
      performance
        .getEntriesByType("resource")
        .some((entry) => entry.name.includes(sensitive)) ||
      cacheNames.some((name) => name.includes(sensitive)) ||
      cacheContains ||
      databaseNames.some((name) => name.includes(sensitive)) ||
      (document.documentElement.textContent ?? "").includes(sensitive)
    );
  }, key);
  const cookieContains = (await page.context().cookies()).some((cookie) =>
    `${cookie.name}\u0000${cookie.value}\u0000${cookie.path}`.includes(key),
  );
  return browserContains || cookieContains;
}

function exactMigrationRequest(request: Request, storeId: string): boolean {
  const url = new URL(request.url());
  return (
    request.method() === "POST" &&
    url.pathname === `/v1/admin/stores/${storeId}/migrate`
  );
}

test("admin can create, edit, and delete a dynamic store", async ({
  page,
  backendWithAdmin,
}) => {
  await loginViaUi(page, backendWithAdmin.baseUrl);
  await page.goto(`${backendWithAdmin.baseUrl}/stores`);
  await expect(page.getByRole("heading", { name: /^stores$/i })).toBeVisible();

  // Create a fresh store; the backend allocates the on-disk dir.
  await page.getByRole("button", { name: /create store/i }).click();
  const createDlg = page.getByRole("dialog", { name: /create store/i });
  await createDlg.getByLabel("Name").fill("scratch");
  await createDlg.getByLabel("Description (optional)").fill("e2e scratch");
  await createDlg.getByRole("button", { name: /^create$/i }).click();
  await expect(createDlg).toBeHidden();
  // Each row's name cell renders the display name plus the
  // backend-assigned store id underneath, so the accessible name is
  // "scratch store_<hex>" and exact-match doesn't apply.
  await expect(page.getByRole("row", { name: /\bscratch\b/ })).toBeVisible();

  // Edit description.
  await page
    .getByRole("row", { name: /\bscratch\b/ })
    .getByRole("button", { name: /edit/i })
    .click();
  const editDlg = page.getByRole("dialog", { name: /edit store: scratch/i });
  await editDlg.getByLabel("Description").fill("renamed-by-test");
  await editDlg.getByRole("button", { name: /^save$/i }).click();
  await expect(editDlg).toBeHidden();
  await expect(page.getByRole("cell", { name: "renamed-by-test" })).toBeVisible();

  // Delete is a simple confirm/cancel — no type-to-confirm gate.
  await page
    .getByRole("row", { name: /\bscratch\b/ })
    .getByRole("button", { name: /delete/i })
    .click();
  const delDlg = page.getByRole("dialog", { name: /delete store: scratch/i });
  const deleteBtn = delDlg.getByRole("button", { name: /^delete$/i });
  await expect(deleteBtn).toBeEnabled();
  await deleteBtn.click();
  await expect(delDlg).toBeHidden();
  await expect(page.getByRole("row", { name: /\bscratch\b/ })).toHaveCount(0);
});

test("encrypted creation requires exact re-entry and sends no mutation on mismatch", async ({
  page,
  backendWithAdmin,
}) => {
  await loginViaUi(page, backendWithAdmin.baseUrl);
  await page.goto(`${backendWithAdmin.baseUrl}/stores`);
  await expect(page.getByRole("heading", { name: /^stores$/i })).toBeVisible();

  let createRequests = 0;
  page.on("request", (request) => {
    const url = new URL(request.url());
    if (request.method() === "POST" && url.pathname === "/v1/admin/stores") {
      createRequests += 1;
    }
  });

  await page.getByRole("button", { name: /create store/i }).click();
  const createDialog = page.getByRole("dialog", { name: /create store/i });
  await createDialog.getByLabel("Name").fill("vault");
  await createDialog.getByLabel("Description (optional)").fill("browser vault");
  await createDialog.getByLabel(/backend/i).selectOption("encrypted");
  await createDialog
    .getByRole("button", { name: /generate and review key/i })
    .click();

  const generatedKey = await displayedGeneratedKey(page);
  const mismatch = `${generatedKey} `;
  await page.getByTestId("store-key-reentry").fill(mismatch);
  await page
    .getByRole("button", { name: /^create encrypted store$/i })
    .click();

  await expect(
    page.getByText(/does not exactly match.*no mutation request was sent/i),
  ).toBeVisible();
  expect(createRequests).toBe(0);
  await expect(storeRow(page, "vault")).toHaveCount(0);
  expect(
    (await page.getByTestId("generated-store-key").innerText()).trim() ===
      generatedKey,
    "the displayed key should remain available after mismatch",
  ).toBe(true);

  await completeExactKeyConfirmation(
    page,
    generatedKey,
    /^create encrypted store$/i,
  );
  expect(createRequests).toBe(1);
  const row = storeRow(page, "vault");
  await expect(row).toBeVisible();
  await expect(row.locator("td").nth(1)).toHaveText("encrypted");
  await expect(row.locator("td").nth(4)).toHaveText("browser vault");
  await expect(row.getByText("unlocked", { exact: true })).toBeVisible();
  expect(await checkedBrowserStateContainsKey(page, generatedKey)).toBe(false);
});

test("real migrations preserve the selected store id, metadata, resource, and lifecycle state", async ({
  page,
  backendWithAdmin,
}) => {
  await loginViaUi(page, backendWithAdmin.baseUrl);
  await page.goto(`${backendWithAdmin.baseUrl}/stores?store=default&keep=yes`);

  const initialRow = storeRow(page, "default");
  await initialRow.getByRole("button", { name: /^edit$/i }).click();
  const editDialog = page.getByRole("dialog", { name: /edit store: default/i });
  await editDialog.getByLabel("Description").fill("migration metadata");
  await editDialog.getByRole("button", { name: /^save$/i }).click();
  await expect(editDialog).toBeHidden();

  await page.goto(
    `${backendWithAdmin.baseUrl}/secrets?store=default&keep=yes`,
  );
  await expect(page.getByTestId("secrets-store-select")).toHaveValue("default");
  await page.getByRole("button", { name: /create resource/i }).click();
  const resourceDialog = page.getByRole("dialog", { name: /create resource/i });
  await resourceDialog
    .getByTestId("create-resource-input")
    .fill("migration/nested-marker");
  await resourceDialog.getByRole("button", { name: /^create$/i }).click();
  await expect(resourceDialog).toBeHidden();
  await expect(page.getByText("migration/nested-marker")).toBeVisible();

  let generatedKeyRequests = 0;
  let encryptedToPlainRequestWasKeyFree = true;
  page.on("request", (request) => {
    const url = new URL(request.url());
    if (
      request.method() === "POST" &&
      url.pathname === "/v1/admin/store-keys"
    ) {
      generatedKeyRequests += 1;
    }
    if (exactMigrationRequest(request, "default")) {
      const body = request.postDataJSON() as {
        backend?: string;
        key_hex?: string;
      };
      if (body.backend === "plain" && "key_hex" in body) {
        encryptedToPlainRequestWasKeyFree = false;
      }
    }
  });

  await page.goto(`${backendWithAdmin.baseUrl}/stores?store=default&keep=yes`);
  const migrationKey = await beginPlainToEncryptedMigration(page, "default");
  await completeExactKeyConfirmation(
    page,
    migrationKey,
    /^migrate to encrypted$/i,
  );

  let row = storeRow(page, "default");
  await expect(row.locator("td").nth(1)).toHaveText("encrypted");
  await expect(row.locator("td").nth(4)).toHaveText("migration metadata");
  await expect(page.getByTestId("store-status-default")).toHaveText("unlocked");
  await expect(page).toHaveURL(
    `${backendWithAdmin.baseUrl}/stores?store=default&keep=yes`,
  );
  expect(generatedKeyRequests).toBe(1);
  expect(await checkedBrowserStateContainsKey(page, migrationKey)).toBe(false);

  await page.goto(
    `${backendWithAdmin.baseUrl}/secrets?store=default&keep=yes`,
  );
  await expect(page.getByTestId("secrets-store-select")).toHaveValue("default");
  await expect(page.getByText("migration/nested-marker")).toBeVisible();
  await expect(page).toHaveURL(
    `${backendWithAdmin.baseUrl}/secrets?store=default&keep=yes`,
  );

  await page.goto(`${backendWithAdmin.baseUrl}/stores?store=default&keep=yes`);
  row = storeRow(page, "default");
  await row.getByRole("button", { name: /migrate to plain/i }).click();
  const plainDialog = page.getByRole("dialog", {
    name: /migrate to plain: default/i,
  });
  await expect(plainDialog).toContainText(
    /plaintext target data under a private stage/i,
  );
  await plainDialog
    .getByRole("button", { name: /^migrate to plain$/i })
    .click();
  await expect(plainDialog).toBeHidden();

  row = storeRow(page, "default");
  await expect(row.locator("td").nth(1)).toHaveText("plain");
  await expect(row.locator("td").nth(4)).toHaveText("migration metadata");
  await expect(page.getByTestId("store-status-default")).toHaveText("ready");
  await expect(page).toHaveURL(
    `${backendWithAdmin.baseUrl}/stores?store=default&keep=yes`,
  );
  expect(generatedKeyRequests).toBe(1);
  expect(encryptedToPlainRequestWasKeyFree).toBe(true);

  await page.goto(
    `${backendWithAdmin.baseUrl}/secrets?store=default&keep=yes`,
  );
  await expect(page.getByTestId("secrets-store-select")).toHaveValue("default");
  await expect(page.getByText("migration/nested-marker")).toBeVisible();
});

test("locked encrypted store gives unlock-first guidance without migration", async ({
  page,
  backendWithAdmin,
}) => {
  await loginViaUi(page, backendWithAdmin.baseUrl);
  await page.goto(`${backendWithAdmin.baseUrl}/stores`);
  const key = await createEncryptedStore(page, "locked-vault");

  const row = storeRow(page, "locked-vault");
  await row.getByRole("button", { name: /^lock$/i }).click();
  const lockDialog = page.getByRole("dialog", {
    name: /lock store: locked-vault/i,
  });
  await lockDialog.getByRole("button", { name: /^lock$/i }).click();
  await expect(lockDialog).toBeHidden();
  await expect(row.locator("td").nth(1)).toHaveText("encrypted");
  await expect(row.getByText("locked", { exact: true })).toBeVisible();

  let migrationRequests = 0;
  page.on("request", (request) => {
    if (exactMigrationRequest(request, "locked-vault")) {
      migrationRequests += 1;
    }
  });
  await row.getByRole("button", { name: /^migrate$/i }).click();
  const guidance = page.getByRole("dialog", {
    name: /migrate store: locked-vault/i,
  });
  await expect(guidance).toContainText(/unlock this store first/i);
  await expect(guidance).toContainText(/no migration request can be sent/i);
  await expect(
    guidance.getByRole("button", { name: /^migrate/i }),
  ).toHaveCount(0);
  expect(migrationRequests).toBe(0);
  expect(await checkedBrowserStateContainsKey(page, key)).toBe(false);
});

test("lost response retains the key and permits same-key retry while the source is unchanged", async ({
  page,
  backendWithAdmin,
}) => {
  await loginViaUi(page, backendWithAdmin.baseUrl);
  await page.goto(`${backendWithAdmin.baseUrl}/stores`);

  let attempts = 0;
  let reconciliationRequests = 0;
  let mutationAttempted = false;
  let submittedSameKey = true;
  let displayedKey: string | null = null;
  page.on("request", (request) => {
    const url = new URL(request.url());
    if (
      mutationAttempted &&
      request.method() === "GET" &&
      url.pathname === "/v1/admin/stores"
    ) {
      reconciliationRequests += 1;
    }
  });
  await page.route("**/v1/admin/stores/default/migrate", async (route) => {
    attempts += 1;
    mutationAttempted = true;
    const body = route.request().postDataJSON() as {
      backend?: string;
      key_hex?: string;
    };
    submittedSameKey =
      submittedSameKey &&
      body.backend === "encrypted" &&
      displayedKey !== null &&
      body.key_hex === displayedKey;
    if (attempts === 1) {
      await route.abort("connectionreset");
      return;
    }
    await route.continue();
  });

  displayedKey = await beginPlainToEncryptedMigration(page, "default");
  await page.getByTestId("store-key-reentry").fill(displayedKey);
  await page
    .getByRole("button", { name: /^migrate to encrypted$/i })
    .click();

  await expect(
    page.getByText(/error or lost response cannot prove that no data changed/i),
  ).toBeVisible();
  expect(
    (await page.getByTestId("generated-store-key").innerText()).trim() ===
      displayedKey,
    "the displayed key should survive an ambiguous outcome",
  ).toBe(true);
  await expect(page.getByTestId("store-key-reentry")).toHaveValue("");
  const retry = page.getByRole("button", {
    name: /retry migrate to encrypted/i,
  });
  await expect(page.getByTestId("store-status-default")).toHaveText("ready");
  await expect.poll(() => reconciliationRequests).toBeGreaterThan(0);
  await expect(retry).toBeEnabled();
  await page.getByTestId("store-key-reentry").fill(displayedKey);
  await retry.click();

  await expect(page.getByTestId("key-confirmation-dialog")).toBeHidden();
  await expect(page.getByTestId("store-status-default")).toHaveText("unlocked");
  expect(attempts).toBe(2);
  expect(submittedSameKey).toBe(true);
  expect(await checkedBrowserStateContainsKey(page, displayedKey)).toBe(false);
});

test("ambiguous failure supports explicit warned discard of the retained key", async ({
  page,
  backendWithAdmin,
}) => {
  await loginViaUi(page, backendWithAdmin.baseUrl);
  await page.goto(`${backendWithAdmin.baseUrl}/stores`);

  let attempts = 0;
  await page.route("**/v1/admin/stores/default/migrate", async (route) => {
    attempts += 1;
    await route.abort("connectionreset");
  });

  const key = await beginPlainToEncryptedMigration(page, "default");
  await page.getByTestId("store-key-reentry").fill(key);
  await page
    .getByRole("button", { name: /^migrate to encrypted$/i })
    .click();
  await expect(
    page.getByRole("button", { name: /discard retained key/i }),
  ).toBeEnabled();
  await page
    .getByRole("button", { name: /discard retained key/i })
    .click();

  await expect(
    page.getByText(/may already protect committed or recoverable data/i),
  ).toBeVisible();
  const discard = page.getByRole("button", { name: /^discard key$/i });
  await expect(discard).toBeDisabled();
  await page
    .getByRole("checkbox", { name: /retained key may still be required/i })
    .check();
  await expect(discard).toBeEnabled();
  await discard.click();

  await expect(page.getByTestId("key-confirmation-dialog")).toBeHidden();
  await expect(page.getByTestId("store-status-default")).toHaveText("ready");
  expect(attempts).toBe(1);
  expect(await checkedBrowserStateContainsKey(page, key)).toBe(false);
});

test("committed migration reconciliation disables source-direction retry and retains custody warning", async ({
  page,
  backendWithAdmin,
}) => {
  await loginViaUi(page, backendWithAdmin.baseUrl);
  await page.goto(`${backendWithAdmin.baseUrl}/stores`);

  let attempts = 0;
  let committedResponseSucceeded = false;
  let requestUsedDisplayedKey = false;
  let displayedKey: string | null = null;
  await page.route("**/v1/admin/stores/default/migrate", async (route) => {
    attempts += 1;
    const body = route.request().postDataJSON() as {
      backend?: string;
      key_hex?: string;
    };
    requestUsedDisplayedKey =
      body.backend === "encrypted" &&
      displayedKey !== null &&
      body.key_hex === displayedKey;
    const response = await route.fetch();
    committedResponseSucceeded = response.ok();
    await route.abort("connectionreset");
  });

  displayedKey = await beginPlainToEncryptedMigration(page, "default");
  await page.getByTestId("store-key-reentry").fill(displayedKey);
  await page
    .getByRole("button", { name: /^migrate to encrypted$/i })
    .click();

  const retry = page.getByRole("button", {
    name: /retry migrate to encrypted/i,
  });
  await expect(retry).toBeDisabled();
  await expect(page.getByText(/already uses the target backend/i)).toBeVisible();
  await expect(
    page.getByText(/retain this key while reconciling the stores list/i),
  ).toBeVisible();
  await expect(
    page.getByText(/external password manager or vault/i),
  ).toBeVisible();
  await expect(page.getByTestId("store-status-default")).toHaveText("unlocked");
  expect(
    (await page.getByTestId("generated-store-key").innerText()).trim() ===
      displayedKey,
    "the displayed key should remain available after committed reconciliation",
  ).toBe(true);
  expect(committedResponseSucceeded).toBe(true);
  expect(requestUsedDisplayedKey).toBe(true);
  expect(attempts).toBe(1);
});

test("rotate-key flow surfaces a fresh key once for an existing encrypted store", async ({
  page,
  backendWithAdmin,
}) => {
  await loginViaUi(page, backendWithAdmin.baseUrl);
  await page.goto(`${backendWithAdmin.baseUrl}/stores`);

  const originalKey = await createEncryptedStore(page, "rotvault");

  await storeRow(page, "rotvault")
    .getByRole("button", { name: /rotate key/i })
    .click();
  const rotateDlg = page.getByRole("dialog", {
    name: /rotate master key: rotvault/i,
  });
  await expect(rotateDlg).toContainText(/remains on the backend host/i);
  await expect(rotateDlg).toContainText(/consumes disk/i);
  await expect(rotateDlg).toContainText(/decryptable with its old key/i);
  await expect(rotateDlg).toContainText(/manual cleanup/i);
  await rotateDlg
    .getByRole("checkbox", { name: /every resource will be re-encrypted/i })
    .check();
  await rotateDlg.getByRole("button", { name: /rotate key/i }).click();

  const reveal = page.getByTestId("key-reveal-dialog");
  await expect(reveal).toBeVisible();
  const rotatedKey = (
    await page.getByTestId("key-reveal-value").innerText()
  ).trim();
  expect(
    /^[0-9a-f]{64}$/.test(rotatedKey),
    "the rotation key should have canonical shape",
  ).toBe(true);
  expect(
    rotatedKey === originalKey,
    "rotation should return a distinct key",
  ).toBe(false);
  await expect(reveal).toContainText(
    /retained old ciphertext consumes backend-host disk/i,
  );
  await expect(reveal).toContainText(/remains decryptable with the old key/i);
  await expect(reveal).toContainText(
    /closing this dialog discards the one-time local display/i,
  );
  await expect(reveal).toContainText(
    /backend cannot know whether I received, saved, or independently verified/i,
  );
  await expect(reveal).not.toContainText(
    /I have saved and independently verified/i,
  );

  await page.getByTestId("key-reveal-ack").check();
  await page.getByTestId("key-reveal-confirm").click();
  await expect(reveal).toBeHidden();
  expect(await checkedBrowserStateContainsKey(page, originalKey)).toBe(false);
  expect(await checkedBrowserStateContainsKey(page, rotatedKey)).toBe(false);

  await page.reload();
  await expect(page.getByTestId("key-reveal-dialog")).toBeHidden();
  const pageContainsRotationKey = await page.evaluate(
    (sensitive) =>
      (document.documentElement.textContent ?? "").includes(sensitive),
    rotatedKey,
  );
  expect(pageContainsRotationKey).toBe(false);
});

// Declarative-vs-dynamic stores no longer exist as a backend
// concept — all stores live in the one JSON registry — so there is
// nothing to test for the "row is locked because nixed it" case.

test("force-delete a non-empty dynamic store after type-to-confirm", async ({
  page,
  backendWithAdmin,
}) => {
  await loginViaUi(page, backendWithAdmin.baseUrl);
  await page.goto(`${backendWithAdmin.baseUrl}/stores`);

  // Create a store, then stuff a secret in via the admin write API
  // before deleting so we exercise the cleanup_hint path.
  await page.getByRole("button", { name: /create store/i }).click();
  const createDlg = page.getByRole("dialog", { name: /create store/i });
  await createDlg.getByLabel("Name").fill("packed");
  await createDlg.getByRole("button", { name: /^create$/i }).click();
  await expect(createDlg).toBeHidden();

  // Put a secret in the store so the force-delete (non-empty) path is
  // exercised. Secret writes are RESTful now — a resource must exist,
  // then a value is written into one of its slots. All admin mutations
  // go through the SPA's synchronizer-token CSRF scheme: capture the
  // `X-CSRF-Token` from each admin response (the cookie is HttpOnly)
  // and replay it on the next request, since the token rotates.
  await page.evaluate(async () => {
    const prime = await fetch("/v1/admin/stores", { credentials: "include" });
    if (!prime.ok) throw new Error(`stores list failed: ${prime.status}`);
    let csrf = prime.headers.get("X-CSRF-Token");
    if (!csrf) throw new Error("no X-CSRF-Token header on admin response");
    const stores = (await prime.json()) as {
      items: Array<{ id: string; name: string }>;
    };
    const packed = stores.items.find((s) => s.name === "packed");
    if (!packed) throw new Error("'packed' store not found in stores list");

    const resRes = await fetch(`/v1/admin/stores/${packed.id}/resources`, {
      method: "POST",
      credentials: "include",
      headers: { "Content-Type": "application/json", "X-CSRF-Token": csrf },
      body: JSON.stringify({ name: "marker-res" }),
    });
    if (!resRes.ok) {
      throw new Error(`resource create failed: ${resRes.status} ${await resRes.text()}`);
    }
    csrf = resRes.headers.get("X-CSRF-Token") ?? csrf;
    const rid = ((await resRes.json()) as { resource: { id: string } }).resource.id;

    const write = await fetch(
      `/v1/admin/stores/${packed.id}/secrets/${rid}/env/marker`,
      {
        method: "PUT",
        credentials: "include",
        headers: { "Content-Type": "application/json", "X-CSRF-Token": csrf },
        body: JSON.stringify({ material_b64: btoa("hello") }),
      },
    );
    if (!write.ok) {
      throw new Error(`secret write failed: ${write.status} ${await write.text()}`);
    }
  });

  // The stores list is cached by tanstack-query, so refresh it
  // before opening the delete dialog — the SPA derives the
  // can_force_delete / can_delete_now action flags from the cached
  // list item, and they only flip once the backend re-reports
  // `has_secrets`.
  await page.reload();

  // Open delete dialog. Because the store now contains a secret, the
  // backend reports `can_delete_now=false` and `can_force_delete=true`
  // so the SPA relabels the submit button to "Force delete" and shows
  // the explicit acknowledgement checkbox.
  await page
    .getByRole("row", { name: /\bpacked\b/ })
    .getByRole("button", { name: /delete/i })
    .click();
  const delDlg = page.getByRole("dialog", { name: /delete store: packed/i });
  const deleteBtn = delDlg.getByRole("button", { name: /^force delete$/i });
  await expect(deleteBtn).toBeDisabled();
  await delDlg.getByLabel(/force delete/i).check();
  await expect(deleteBtn).toBeEnabled();
  await deleteBtn.click();

  // The backend returns a cleanup_hint when the data dir is non-empty;
  // the SPA surfaces it in a follow-up dialog. Dismiss it.
  const storeDeletedDlg = page.getByRole("dialog", { name: /store deleted/i });
  await expect(storeDeletedDlg).toBeVisible();
  await expect(storeDeletedDlg.getByText(/packed/).first()).toBeVisible();
  await storeDeletedDlg.getByText("Close", { exact: true }).click();
  await expect(page.getByRole("row", { name: /\bpacked\b/ })).toHaveCount(0);
});

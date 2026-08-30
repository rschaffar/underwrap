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
import { test, expect, loginViaUi } from "./fixtures";

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

test("creating an encrypted store reveals the master key once and leaves no traces in storage", async ({
  page,
  backendWithAdmin,
}) => {
  await loginViaUi(page, backendWithAdmin.baseUrl);
  await page.goto(`${backendWithAdmin.baseUrl}/stores`);
  await expect(page.getByRole("heading", { name: /^stores$/i })).toBeVisible();

  await page.getByRole("button", { name: /create store/i }).click();
  const createDlg = page.getByRole("dialog", { name: /create store/i });
  await createDlg.getByLabel("Name").fill("vault");
  await createDlg
    .getByLabel(/backend/i)
    .selectOption("encrypted");
  // The free-form key file path field is gone — the backend chooses
  // where to put it.
  await expect(createDlg.getByLabel(/key file/i)).toBeHidden();
  await createDlg.getByRole("button", { name: /^create$/i }).click();

  // The reveal modal supersedes the create dialog.
  const reveal = page.getByTestId("key-reveal-dialog");
  await expect(reveal).toBeVisible();
  const keyValue = (
    await page.getByTestId("key-reveal-value").innerText()
  ).trim();
  expect(keyValue).toMatch(/^[0-9a-f]{64}$/);

  // Confirm button is disabled until the acknowledgement box is ticked.
  const confirmBtn = page.getByTestId("key-reveal-confirm");
  await expect(confirmBtn).toBeDisabled();
  await page.getByTestId("key-reveal-ack").check();
  await expect(confirmBtn).toBeEnabled();
  await confirmBtn.click();
  await expect(reveal).toBeHidden();
  await expect(page.getByRole("row", { name: /\bvault\b/ })).toBeVisible();

  // Storage must be clean: no localStorage / sessionStorage entry,
  // no cookie, and the URL must not carry the key value.
  const storageSnapshot = await page.evaluate(() => ({
    local: Object.entries(localStorage),
    session: Object.entries(sessionStorage),
    cookie: document.cookie,
    href: window.location.href,
  }));
  const serialised = JSON.stringify(storageSnapshot);
  expect(
    serialised,
    "key value must not appear in storage, cookies, or the URL after the reveal flow",
  ).not.toContain(keyValue);
});

test("rotate-key flow surfaces a fresh key once for an existing encrypted store", async ({
  page,
  backendWithAdmin,
}) => {
  await loginViaUi(page, backendWithAdmin.baseUrl);
  await page.goto(`${backendWithAdmin.baseUrl}/stores`);

  // Create an encrypted store first.
  await page.getByRole("button", { name: /create store/i }).click();
  const createDlg = page.getByRole("dialog", { name: /create store/i });
  await createDlg.getByLabel("Name").fill("rotvault");
  await createDlg
    .getByLabel(/backend/i)
    .selectOption("encrypted");
  await createDlg.getByRole("button", { name: /^create$/i }).click();
  await page.getByTestId("key-reveal-ack").check();
  const originalKey = (
    await page.getByTestId("key-reveal-value").innerText()
  ).trim();
  await page.getByTestId("key-reveal-confirm").click();

  // Trigger a rotate via the dedicated table action.
  await page
    .getByRole("row", { name: /rotvault/ })
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
  expect(rotatedKey).toMatch(/^[0-9a-f]{64}$/);
  expect(rotatedKey).not.toEqual(originalKey);
  await expect(reveal).toContainText(/retained old ciphertext consumes backend-host disk/i);
  await expect(reveal).toContainText(/remains decryptable with the old key/i);
  await expect(reveal).toContainText(/closing this dialog discards the one-time local display/i);
  await expect(reveal).toContainText(
    /backend cannot know whether I received, saved, or independently verified/i,
  );
  await expect(reveal).not.toContainText(/I have saved and independently verified/i);

  await page.getByTestId("key-reveal-ack").check();
  await page.getByTestId("key-reveal-confirm").click();
  await expect(reveal).toBeHidden();

  const browserState = await page.evaluate(() => ({
    local: Object.entries(localStorage),
    session: Object.entries(sessionStorage),
    cookie: document.cookie,
    href: window.location.href,
    resources: performance.getEntriesByType("resource").map((entry) => entry.name),
  }));
  const cookies = await page.context().cookies();
  const storesResponse = await page.request.get(
    `${backendWithAdmin.baseUrl}/v1/admin/stores`,
  );
  expect(storesResponse.ok()).toBe(true);
  const laterState = JSON.stringify({
    browserState,
    cookies,
    stores: await storesResponse.text(),
  });
  expect(laterState).not.toContain(originalKey);
  expect(laterState).not.toContain(rotatedKey);

  await page.reload();
  await expect(page.getByTestId("key-reveal-dialog")).toBeHidden();
  expect(await page.content()).not.toContain(rotatedKey);
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

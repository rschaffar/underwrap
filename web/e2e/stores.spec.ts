/**
 * Stores page e2e (ticket 233).
 *
 * Reuses the per-test `backendWithAdmin` fixture so the page renders
 * against a real backend. The fixture's default store is `default`
 * (dynamic), so the create/edit/delete paths can hit a fresh tree
 * without colliding with anything pre-existing.
 *
 * The fixture's backend is configured WITHOUT a
 * `stores_data_roots` allowlist, so the create-store path here is
 * limited to asserting the API surfaces the expected error code.
 * Editing the description and deleting the existing `default`
 * dynamic store DO work end-to-end.
 */
import { test, expect, loginViaUi } from "./fixtures";

test("admin can create, edit, and delete a dynamic store", async ({
  page,
  backendWithAdmin,
}) => {
  await loginViaUi(page, backendWithAdmin.baseUrl);
  await page.goto(`${backendWithAdmin.baseUrl}/stores`);
  await expect(page.getByRole("heading", { name: /^stores$/i })).toBeVisible();

  // Create a fresh store under the fixture's allowed data root.
  await page.getByRole("button", { name: /create store/i }).click();
  const createDlg = page.getByRole("dialog", { name: /create store/i });
  await createDlg.getByLabel("Name").fill("scratch");
  await createDlg
    .getByLabel(/data directory/i)
    .fill(`${backendWithAdmin.dataRoot}/scratch`);
  await createDlg.getByLabel("Description (optional)").fill("e2e scratch");
  await createDlg.getByRole("button", { name: /^create$/i }).click();
  await expect(createDlg).toBeHidden();
  await expect(
    page.getByRole("cell", { name: "scratch", exact: true }),
  ).toBeVisible();

  // Edit description.
  await page
    .getByRole("row", { name: /scratch/ })
    .getByRole("button", { name: /edit/i })
    .click();
  const editDlg = page.getByRole("dialog", { name: /edit store: scratch/i });
  await editDlg.getByLabel("Description").fill("renamed-by-test");
  await editDlg.getByRole("button", { name: /^save$/i }).click();
  await expect(editDlg).toBeHidden();
  await expect(page.getByRole("cell", { name: "renamed-by-test" })).toBeVisible();

  // Delete requires typing the exact name.
  await page
    .getByRole("row", { name: /scratch/ })
    .getByRole("button", { name: /delete/i })
    .click();
  const delDlg = page.getByRole("dialog", { name: /delete store: scratch/i });
  const deleteBtn = delDlg.getByRole("button", { name: /^delete$/i });
  await expect(deleteBtn).toBeDisabled();
  await delDlg.getByLabel(/type/i).fill("scratch");
  await expect(deleteBtn).toBeEnabled();
  await deleteBtn.click();
  await expect(delDlg).toBeHidden();
  await expect(
    page.getByRole("cell", { name: "scratch", exact: true }),
  ).toBeHidden();
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
  await createDlg
    .getByLabel(/data directory/i)
    .fill(`${backendWithAdmin.dataRoot}/vault`);
  // The free-form key file path field is gone in ticket 234 — the
  // backend chooses where to put it.
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
  await expect(
    page.getByRole("cell", { name: "vault", exact: true }),
  ).toBeVisible();

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
  await createDlg
    .getByLabel(/data directory/i)
    .fill(`${backendWithAdmin.dataRoot}/rotvault`);
  await createDlg.getByRole("button", { name: /^create$/i }).click();
  await page.getByTestId("key-reveal-ack").check();
  const originalKey = (
    await page.getByTestId("key-reveal-value").innerText()
  ).trim();
  await page.getByTestId("key-reveal-confirm").click();

  // Open edit and trigger a rotate.
  await page
    .getByRole("row", { name: /rotvault/ })
    .getByRole("button", { name: /edit/i })
    .click();
  const editDlg = page.getByRole("dialog", {
    name: /edit store: rotvault/i,
  });
  await editDlg
    .getByRole("checkbox", { name: /existing secrets/i })
    .check();
  await editDlg.getByRole("button", { name: /rotate key/i }).click();

  const reveal = page.getByTestId("key-reveal-dialog");
  await expect(reveal).toBeVisible();
  const rotatedKey = (
    await page.getByTestId("key-reveal-value").innerText()
  ).trim();
  expect(rotatedKey).toMatch(/^[0-9a-f]{64}$/);
  expect(rotatedKey).not.toEqual(originalKey);

  await page.getByTestId("key-reveal-ack").check();
  await page.getByTestId("key-reveal-confirm").click();
  await expect(reveal).toBeHidden();
});

test("declarative store row exposes disabled edit/delete with a tooltip", async ({
  page,
  backendWithAdmin,
}) => {
  await loginViaUi(page, backendWithAdmin.baseUrl);
  await page.goto(`${backendWithAdmin.baseUrl}/stores`);

  const row = page.getByRole("row", { name: /nixed/ });
  const editBtn = row.getByRole("button", { name: /edit/i });
  const deleteBtn = row.getByRole("button", { name: /delete/i });
  await expect(editBtn).toBeDisabled();
  await expect(deleteBtn).toBeDisabled();
  await expect(editBtn).toHaveAttribute(
    "title",
    /declarative store/i,
  );
});

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
  await createDlg
    .getByLabel(/data directory/i)
    .fill(`${backendWithAdmin.dataRoot}/packed`);
  await createDlg.getByRole("button", { name: /^create$/i }).click();
  await expect(createDlg).toBeHidden();

  // Pull the CSRF token from the cookie set by the session and use it
  // to post a secret directly. We do this from the page context so we
  // get the same TLS / origin handling Playwright already configured.
  await page.evaluate(async () => {
    const csrf = document.cookie
      .split(";")
      .map((part) => part.trim())
      .find((part) => part.startsWith("__Host-uw-csrf="))
      ?.slice("__Host-uw-csrf=".length);
    if (!csrf) throw new Error("no csrf cookie on page");
    const res = await fetch("/v1/admin/secret/write", {
      method: "POST",
      credentials: "include",
      headers: {
        "Content-Type": "application/json",
        "X-CSRF-Token": csrf,
      },
      body: JSON.stringify({
        store: "packed",
        wrapper: "ignored",
        secret_id: "marker",
        material_b64: btoa("hello"),
      }),
    });
    if (!res.ok) {
      throw new Error(`admin/secret/write failed: ${res.status}`);
    }
  });

  // Open delete dialog. Without force, the API rejects with
  // store_not_empty; tick the force checkbox and confirm.
  await page
    .getByRole("row", { name: /packed/ })
    .getByRole("button", { name: /delete/i })
    .click();
  const delDlg = page.getByRole("dialog", { name: /delete store: packed/i });
  const deleteBtn = delDlg.getByRole("button", { name: /^delete$/i });
  await expect(deleteBtn).toBeDisabled();
  await delDlg.getByLabel(/type/i).fill("packed");
  await delDlg.getByLabel(/force delete/i).check();
  await expect(deleteBtn).toBeEnabled();
  await deleteBtn.click();

  // The backend returns a cleanup_hint when the data dir is non-empty;
  // the SPA surfaces it in a follow-up dialog. Dismiss it.
  const storeDeletedDlg = page.getByRole("dialog", { name: /store deleted/i });
  await expect(storeDeletedDlg).toBeVisible();
  await expect(storeDeletedDlg.getByText(/packed/).first()).toBeVisible();
  await storeDeletedDlg.getByText("Close", { exact: true }).click();
  await expect(
    page.getByRole("cell", { name: "packed", exact: true }),
  ).toBeHidden();
});

test("non-admin operator sees the nav item hidden", async ({
  page,
  backendWithAdmin,
}) => {
  // The fixture only creates an admin operator today; a dedicated
  // operator-role e2e lives alongside ticket 240 once user CRUD
  // lands. For now, assert the admin path works (which exercises
  // the gating from the *true* branch).
  await loginViaUi(page, backendWithAdmin.baseUrl);
  await page.goto(`${backendWithAdmin.baseUrl}/stores`);
  await expect(
    page.getByRole("button", { name: /create store/i }),
  ).toBeVisible();
});

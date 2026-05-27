/**
 * Wrapper permissions page e2e.
 *
 * Exercises the wrapper/resource-pair + wrapper-permission model
 * end-to-end from the SPA: pair creation through the dialog,
 * cell-editor flows for each of the four modes, removing a permission,
 * and a refresh that preserves the URL `?store=` selection.
 */
import { test, expect, loginViaUi } from "./fixtures";

// Admin mutations are driven from inside the page via `fetch(...,
// { credentials: "include" })` so they share the SPA's exact session.
// The CSRF cookie is HttpOnly, so the synchronizer token can't be read
// from `document.cookie`; instead — exactly like the SPA's client — we
// capture the `X-CSRF-Token` response header from a priming admin GET and
// replay it on the mutating request. (Playwright's `page.request` uses a
// separate cookie jar and trips the CSRF check, hence the in-page fetch.)

type AdminFetch = {
  method: "POST" | "PUT";
  path: string;
  body: unknown;
};

async function adminFetchJson<T>(
  page: import("@playwright/test").Page,
  req: AdminFetch,
): Promise<T> {
  return page.evaluate(async ({ method, path, body }) => {
    const prime = await fetch("/v1/admin/stores", { credentials: "include" });
    if (!prime.ok) {
      throw new Error(`csrf prime GET failed ${prime.status} — login first`);
    }
    const csrf = prime.headers.get("X-CSRF-Token");
    if (!csrf) throw new Error("no X-CSRF-Token header on admin response");
    const res = await fetch(path, {
      method,
      credentials: "include",
      headers: { "Content-Type": "application/json", "X-CSRF-Token": csrf },
      body: JSON.stringify(body),
    });
    if (!res.ok) {
      throw new Error(`${method} ${path} failed ${res.status}: ${await res.text()}`);
    }
    return (await res.json()) as T;
  }, req);
}

/** Create a resource (random backend id) and return that id. */
async function createResource(
  page: import("@playwright/test").Page,
  store: string,
  name: string,
): Promise<string> {
  const body = await adminFetchJson<{ resource: { id: string } }>(page, {
    method: "POST",
    path: `/v1/admin/stores/${store}/resources`,
    body: { name },
  });
  return body.resource.id;
}

/**
 * Create a resource and a wrapper/resource pair against it. Returns the
 * resource id, which addresses the table cells
 * (`wrappers-cell-<wrapper>-<resource_id>-<client>`).
 */
async function seedPair(
  page: import("@playwright/test").Page,
  store: string,
  wrapper: string,
  resourceName: string,
): Promise<string> {
  const resourceId = await createResource(page, store, resourceName);
  await adminFetchJson(page, {
    method: "POST",
    path: `/v1/admin/stores/${store}/pairs`,
    body: { wrapper, resource_id: resourceId },
  });
  return resourceId;
}

/** Upsert a wrapper permission via the REST endpoint. */
async function upsertPermission(
  page: import("@playwright/test").Page,
  store: string,
  clientInstance: string,
  wrapper: string,
  resourceId: string,
  mode: { kind: string; until_epoch?: number },
  writable: boolean,
): Promise<void> {
  await adminFetchJson(page, {
    method: "PUT",
    path: `/v1/admin/stores/${store}/wrapper-permissions/${clientInstance}/${wrapper}/${resourceId}`,
    body: { mode, writable },
  });
}

test("wrappers page loads and shows the empty-pairs message on a fresh store", async ({
  page,
  backendWithAdmin,
}) => {
  await loginViaUi(page, backendWithAdmin.baseUrl);
  await page.goto(`${backendWithAdmin.baseUrl}/wrappers`);

  await expect(
    page.getByRole("heading", { name: /wrapper permissions/i }),
  ).toBeVisible();

  await expect(
    page.getByText(/no wrapper\/resource pairs are registered/i),
  ).toBeVisible();
});

test("cell editor cycles a permission through every mode (incl. unsafe-debug confirm) and then removes it", async ({
  page,
  backendWithAdmin,
}) => {
  await loginViaUi(page, backendWithAdmin.baseUrl);

  // Seed a pair record so the table has a row to drive permissions against.
  const resourceId = await seedPair(
    page,
    "default",
    "gh-authorized",
    "github/token",
  );
  // Seed an initial permission so the cell starts non-empty.
  await upsertPermission(
    page,
    "default",
    "dev-laptop",
    "gh-authorized",
    resourceId,
    { kind: "locked" },
    false,
  );

  await page.goto(`${backendWithAdmin.baseUrl}/wrappers?store=default`);

  const cell = page.getByTestId(
    `wrappers-cell-gh-authorized-${resourceId}-dev-laptop`,
  );
  await expect(cell).toBeVisible();
  await expect(cell.getByTestId("mode-locked")).toBeVisible();

  async function setMode(mode: string) {
    await cell.click();
    await expect(page.getByTestId("mode-radio-group")).toBeVisible();
    await page
      .getByTestId(`mode-radio-${mode}`)
      .click({ force: true });
    if (mode === "unsafe-debug-until") {
      // Picking unsafe-debug-until opens a danger-confirm gate; the edit
      // dialog only switches into the mode once it is confirmed.
      await page.getByTestId("confirm-danger-confirm").click();
    }
    await page.getByTestId("permission-save").click();
    await expect(page.getByTestId("permission-save")).toBeHidden();
  }

  await setMode("unlocked");
  await expect(cell.getByTestId("mode-unlocked")).toBeVisible();

  await setMode("unlocked-until");
  await expect(cell.getByTestId("mode-unlocked-until")).toBeVisible();

  await setMode("unsafe-debug-until");
  await expect(cell.getByTestId("mode-unsafe-debug-until")).toBeVisible();

  await setMode("locked");
  await expect(cell.getByTestId("mode-locked")).toBeVisible();

  // Remove the permission. The pair row remains visible because the
  // pair record is independent of any permissions on it.
  await cell.click();
  const dlg = page.getByRole("dialog");
  await dlg.getByTestId("permission-remove").click();
  await expect(dlg).toBeHidden();
});

test("admin can create a wrapper/resource pair through the dialog", async ({
  page,
  backendWithAdmin,
}) => {
  await loginViaUi(page, backendWithAdmin.baseUrl);
  await page.goto(`${backendWithAdmin.baseUrl}/wrappers?store=default`);

  await page.getByTestId("wrappers-create-pair").click();
  const dlg = page.getByRole("dialog", { name: /create wrapper\/resource pair/i });
  await expect(dlg).toBeVisible();
  await dlg.getByLabel(/wrapper name/i).fill("gh-authorized");
  await dlg.getByTestId("pair-new-resource").fill("github/token");
  await dlg.getByRole("button", { name: /^create$/i }).click();
  await expect(dlg).toBeHidden();

  // The pair row is now visible in the table. Match on text content with
  // exact:true so the delete button's "Delete pair gh-authorized /
  // github/token" aria-label (not text content) doesn't interfere.
  await expect(
    page.getByText("gh-authorized", { exact: true }),
  ).toBeVisible();
  await expect(page.getByText("github/token", { exact: true })).toBeVisible();
});

test("raw policy viewer surfaces the live PolicyDocument and writable-permission count", async ({
  page,
  backendWithAdmin,
}) => {
  await loginViaUi(page, backendWithAdmin.baseUrl);
  await page.goto(`${backendWithAdmin.baseUrl}/wrappers?store=default`);

  // The writable-permission count badge lives in the page header now
  // (always visible), not inside the raw-policy dialog.
  await expect(page.getByTestId("writable-permissions-badge")).toHaveText(
    /0 writable of 0/i,
  );

  await page.getByRole("button", { name: /view raw policy/i }).click();
  const dlg = page.getByRole("dialog", { name: /raw policy: default/i });
  await expect(dlg).toBeVisible();
  await expect(dlg.getByTestId("policy-json")).toContainText('"permissions"');

  await dlg.getByText("Close", { exact: true }).click();
  await expect(dlg).toBeHidden();

  // Seed a writable permission. The raw policy reads policy.permissions
  // directly and the header badge counts them, so a resource + writable
  // permission is enough to bump the count to 1.
  const resourceId = await createResource(page, "default", "github/token");
  await upsertPermission(
    page,
    "default",
    "dev-laptop",
    "gh-authorized",
    resourceId,
    { kind: "unlocked" },
    true,
  );

  await page.reload();
  await expect(page.getByTestId("writable-permissions-badge")).toHaveText(
    /1 writable of 1/i,
  );

  await page.getByRole("button", { name: /view raw policy/i }).click();
  const dlg2 = page.getByRole("dialog", { name: /raw policy: default/i });
  await expect(dlg2.getByTestId("policy-json")).toContainText('"writable": true');
});

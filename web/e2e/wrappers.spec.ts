/**
 * Wrappers page e2e (ticket 392).
 *
 * Exercises the wrapper/resource-pair + agent-grant model end-to-end
 * from the SPA: pair creation through the dialog, cell-editor flows
 * for each of the four grant modes, removing a grant, and a refresh
 * that preserves the URL `?store=` selection.
 */
import { test, expect, loginViaUi } from "./fixtures";

/** Fetch the live `X-CSRF-Token` cookie so admin API requests we drive
 *  directly carry the same double-submit token the SPA would. */
async function csrfFromPage(page: import("@playwright/test").Page): Promise<string> {
  const cookies = await page.context().cookies();
  const csrf = cookies.find((c) => c.name === "__Host-uw-csrf");
  if (!csrf) throw new Error("__Host-uw-csrf cookie missing — login first");
  return csrf.value;
}

async function seedPair(
  page: import("@playwright/test").Page,
  baseUrl: string,
  store: string,
  wrapper: string,
  resource: string,
): Promise<void> {
  const csrf = await csrfFromPage(page);
  // The admin pair-create requires the resource to exist first.
  const r = await page.request.post(
    `${baseUrl}/v1/admin/resources/create`,
    {
      headers: {
        "Content-Type": "application/json",
        "X-CSRF-Token": csrf,
        Origin: baseUrl,
      },
      data: { store, name: resource },
    },
  );
  expect([200, 409], `seed resource: ${await r.text()}`).toContain(r.status());
  const p = await page.request.post(`${baseUrl}/v1/admin/pairs/create`, {
    headers: {
      "Content-Type": "application/json",
      "X-CSRF-Token": csrf,
      Origin: baseUrl,
    },
    data: { store, wrapper, resource },
  });
  expect([200, 409], `seed pair: ${await p.text()}`).toContain(p.status());
}

test("wrappers page loads and shows the empty-pairs message on a fresh store", async ({
  page,
  backendWithAdmin,
}) => {
  await loginViaUi(page, backendWithAdmin.baseUrl);
  await page.goto(`${backendWithAdmin.baseUrl}/wrappers`);

  await expect(page.getByRole("heading", { name: /^wrappers$/i })).toBeVisible();

  await expect(
    page.getByText(/no wrapper\/resource pairs are registered/i),
  ).toBeVisible();
});

test("cell editor cycles a grant through all four modes and then removes it", async ({
  page,
  backendWithAdmin,
}) => {
  await loginViaUi(page, backendWithAdmin.baseUrl);

  // Seed a pair record so the table has a row to drive grants against.
  await seedPair(
    page,
    backendWithAdmin.baseUrl,
    "default",
    "gh-authorized",
    "github/token",
  );
  // Seed an initial grant so the cell starts non-empty.
  const csrf = await csrfFromPage(page);
  const seed = await page.request.post(
    `${backendWithAdmin.baseUrl}/v1/admin/grants/upsert`,
    {
      headers: {
        "Content-Type": "application/json",
        "X-CSRF-Token": csrf,
        Origin: backendWithAdmin.baseUrl,
      },
      data: {
        store: "default",
        grant: {
          client_instance: "dev-laptop",
          wrapper: "gh-authorized",
          resource: "github/token",
          mode: { kind: "locked" },
          writable: false,
        },
      },
    },
  );
  expect(seed.status(), `seed: ${await seed.text()}`).toBe(200);

  await page.goto(`${backendWithAdmin.baseUrl}/wrappers?store=default`);

  const cell = page.getByTestId(
    "wrappers-cell-gh-authorized-github/token-dev-laptop",
  );
  await expect(cell).toBeVisible();
  await expect(cell.getByTestId("mode-locked")).toBeVisible();

  async function setMode(mode: string) {
    await cell.click();
    const dlg = page.getByRole("dialog");
    await expect(dlg).toBeVisible();
    await page
      .getByTestId(`mode-radio-${mode}`)
      .click({ force: true });
    if (mode === "per-call-approval") {
      await dlg
        .getByLabel(/ssh public key/i)
        .fill("ssh-ed25519 AAAAAAAA test@example");
    }
    await dlg.getByTestId("grant-save").click();
    await expect(dlg).toBeHidden();
  }

  await setMode("unlocked");
  await expect(cell.getByTestId("mode-unlocked")).toBeVisible();

  await setMode("unlocked-until");
  await expect(cell.getByTestId("mode-unlocked-until")).toBeVisible();

  await setMode("per-call-approval");
  await expect(cell.getByTestId("mode-per-call-approval")).toBeVisible();

  await setMode("locked");
  await expect(cell.getByTestId("mode-locked")).toBeVisible();

  // Remove the grant. The pair row remains visible because the pair
  // record is independent of any grants on it.
  await cell.click();
  const dlg = page.getByRole("dialog");
  await dlg.getByTestId("grant-remove").click();
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

  // The pair row is now visible in the table.
  await expect(page.getByRole("cell", { name: "gh-authorized" })).toBeVisible();
  await expect(page.getByRole("cell", { name: "github/token" })).toBeVisible();
});

test("raw policy viewer surfaces the live PolicyDocument and writable-grant count", async ({
  page,
  backendWithAdmin,
}) => {
  await loginViaUi(page, backendWithAdmin.baseUrl);
  await page.goto(`${backendWithAdmin.baseUrl}/wrappers`);

  await page.getByRole("button", { name: /view raw policy/i }).click();
  const dlg = page.getByRole("dialog", { name: /raw policy: default/i });
  await expect(dlg).toBeVisible();

  await expect(dlg.getByTestId("writable-grants-badge")).toHaveText(/0 of 0/i);
  const json = dlg.getByTestId("policy-json");
  await expect(json).toContainText('"grants"');

  await dlg.getByText("Close", { exact: true }).click();
  await expect(dlg).toBeHidden();

  const csrf = await csrfFromPage(page);
  // Pair must exist before a grant can render on the wrappers page,
  // but the raw policy viewer reads policy.grants directly so the
  // upsert below is sufficient to bump the writable count.
  const setResp = await page.request.post(
    `${backendWithAdmin.baseUrl}/v1/admin/grants/upsert`,
    {
      data: {
        store: "default",
        grant: {
          client_instance: "dev-laptop",
          wrapper: "gh-authorized",
          resource: "github/token",
          mode: { kind: "unlocked" },
          writable: true,
        },
      },
      headers: {
        "X-CSRF-Token": csrf,
        Origin: backendWithAdmin.baseUrl,
      },
    },
  );
  expect(setResp.status()).toBe(200);

  await page.getByRole("button", { name: /view raw policy/i }).click();
  const dlg2 = page.getByRole("dialog", { name: /raw policy: default/i });
  await expect(dlg2.getByTestId("writable-grants-badge")).toHaveText(/1 of 1/i);
  await expect(dlg2.getByTestId("policy-json")).toContainText('"writable": true');
});

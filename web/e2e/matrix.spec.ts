/**
 * Matrix page e2e (ticket 250).
 *
 * Exercises the new agent-grant model end-to-end from the SPA: the
 * writes-enabled toggle, cell-editor flows for each of the four
 * modes, removing a grant, and a refresh that preserves the URL
 * `?store=` selection.
 */
import { test, expect, loginViaUi } from "./fixtures";

/** Fetch the live `X-CSRF-Token` cookie from the page so the admin
 *  API requests we drive directly carry the same double-submit token
 *  the SPA would. */
async function csrfFromPage(page: import("@playwright/test").Page): Promise<string> {
  const cookies = await page.context().cookies();
  const csrf = cookies.find((c) => c.name === "__Host-uw-csrf");
  if (!csrf) throw new Error("__Host-uw-csrf cookie missing — login first");
  return csrf.value;
}

test("matrix page loads, shows writes toggle, and renders empty for an empty catalog", async ({
  page,
  backendWithAdmin,
}) => {
  await loginViaUi(page, backendWithAdmin.baseUrl);
  await page.goto(`${backendWithAdmin.baseUrl}/matrix`);

  await expect(page.getByRole("heading", { name: /^matrix$/i })).toBeVisible();
  await expect(page.getByTestId("writes-toggle")).toBeVisible();

  // Empty fixture: no agents register, so the catalog is empty.
  await expect(
    page.getByText(/matrix is empty for this store/i),
  ).toBeVisible();
});

test("toggling writes-enabled flips the banner and persists across reload", async ({
  page,
  backendWithAdmin,
}) => {
  await loginViaUi(page, backendWithAdmin.baseUrl);
  await page.goto(`${backendWithAdmin.baseUrl}/matrix`);

  const toggle = page.getByTestId("writes-toggle-input");
  await expect(toggle).not.toBeChecked();
  // The toggle is controlled — `checked` reflects the cached query.
  // Click and then poll for the post-refetch state instead of
  // using `.check()`, which asserts the value flipped synchronously.
  await toggle.click();
  await expect(toggle).toBeChecked();
  await expect(
    page.getByText(/writes enabled globally\. disable when finished/i),
  ).toBeVisible();

  // Reload: state must come from the backend, not the SPA cache.
  await page.reload();
  await expect(page.getByTestId("writes-toggle-input")).toBeChecked();

  // Flip back off; the banner disappears.
  await page.getByTestId("writes-toggle-input").click();
  await expect(page.getByTestId("writes-toggle-input")).not.toBeChecked();
  await expect(
    page.getByText(/writes enabled globally\. disable when finished/i),
  ).toBeHidden();
});

test("cell editor cycles a grant through all four modes and then removes it", async ({
  page,
  backendWithAdmin,
}) => {
  await loginViaUi(page, backendWithAdmin.baseUrl);

  // Seed one grant directly via the admin API so the matrix has
  // something to render. The grant's tuple
  // (wrapper, host, user) = (gh-authorized, alpha, robot) is the
  // row we'll cycle in the UI.
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
          wrapper: "gh-authorized",
          host: "alpha",
          user: "robot",
          mode: { kind: "locked" },
        },
      },
    },
  );
  expect(seed.status(), `seed: ${await seed.text()}`).toBe(200);

  await page.goto(`${backendWithAdmin.baseUrl}/matrix?store=default`);

  const cell = page.getByTestId("matrix-cell-gh-authorized-alpha-robot");
  await expect(cell).toBeVisible();
  await expect(cell.getByTestId("mode-locked")).toBeVisible();

  // Cycle the cell through every mode in turn.
  async function setMode(mode: string) {
    await cell.click();
    const dlg = page.getByRole("dialog");
    await expect(dlg).toBeVisible();
    const radios = dlg.getByTestId("mode-radio-group");
    // The radio inputs are sr-only; clicking the label is the
    // intended gesture, but Playwright's strict mode wants a
    // single result. Click the input itself with `force: true`
    // to bypass the visibility check.
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

  // Remove the grant. With no catalog entries seeded by an agent
  // and no other grants on this principal, the row+column collapse
  // and the matrix falls back to the empty-state copy.
  await cell.click();
  const dlg = page.getByRole("dialog");
  await dlg.getByTestId("grant-remove").click();
  await expect(dlg).toBeHidden();
  await expect(cell).toBeHidden();
  await expect(
    page.getByText(/matrix is empty for this store/i),
  ).toBeVisible();
});

test("raw policy viewer surfaces the live PolicyDocument and the writes-enabled badge (ticket 301)", async ({
  page,
  backendWithAdmin,
}) => {
  await loginViaUi(page, backendWithAdmin.baseUrl);
  await page.goto(`${backendWithAdmin.baseUrl}/matrix`);

  // Open the viewer. The button is exposed on the Matrix header.
  await page.getByRole("button", { name: /view raw policy/i }).click();
  const dlg = page.getByRole("dialog", { name: /raw policy: default/i });
  await expect(dlg).toBeVisible();

  // Initial state: empty grants, writes_enabled = false.
  await expect(dlg.getByTestId("writes-enabled-badge")).toHaveText(/^off$/i);
  const json = dlg.getByTestId("policy-json");
  await expect(json).toContainText('"grants"');
  await expect(json).toContainText('"writes_enabled": false');

  // Close, flip writes-enabled on, reopen — the badge tracks the
  // backend state, not a cached view. The dialog has two "Close"
  // buttons (footer button + Radix's X corner button); the footer
  // one carries the matching accessible text content.
  await dlg.getByText("Close", { exact: true }).click();
  await expect(dlg).toBeHidden();
  await page.getByTestId("writes-toggle-input").click();
  await expect(page.getByTestId("writes-toggle-input")).toBeChecked();

  await page.getByRole("button", { name: /view raw policy/i }).click();
  const dlg2 = page.getByRole("dialog", { name: /raw policy: default/i });
  await expect(dlg2.getByTestId("writes-enabled-badge")).toHaveText(/^on$/i);
  await expect(dlg2.getByTestId("policy-json")).toContainText(
    '"writes_enabled": true',
  );
});

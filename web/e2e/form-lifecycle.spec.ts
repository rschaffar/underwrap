import { expect, loginViaUi, test } from "./fixtures";

function deferred() {
  let resolve!: () => void;
  const promise = new Promise<void>((done) => { resolve = done; });
  return { promise, resolve };
}

test("Create resource owns its draft through a slow request and refresh", async ({ page, backendWithAdmin }) => {
  const requestSeen = deferred();
  const releaseRequest = deferred();
  const refreshSeen = deferred();
  const releaseRefresh = deferred();
  let holdRefresh = false;
  let submissions = 0;
  await page.route("**/v1/admin/stores/default/resources", async (route) => {
    if (route.request().method() !== "POST") return route.continue();
    submissions++;
    requestSeen.resolve();
    await releaseRequest.promise;
    await route.continue();
  });
  await page.route("**/v1/admin/stores/default/secrets-view", async (route) => {
    if (holdRefresh) {
      refreshSeen.resolve();
      await releaseRefresh.promise;
    }
    await route.continue();
  });

  await loginViaUi(page, backendWithAdmin.baseUrl);
  await page.goto(`${backendWithAdmin.baseUrl}/secrets?store=default`);
  const open = page.getByRole("button", { name: "Create resource", exact: true });
  await open.click();
  const dialog = page.getByRole("dialog", { name: "Create resource", exact: true });
  await dialog.getByLabel("Resource name").fill("lifecycle-first");
  await dialog.getByRole("button", { name: "Create", exact: true }).click();
  await requestSeen.promise;

  async function checkPending() {
    const cancel = dialog.getByRole("button", { name: "Cancel", exact: true });
    const close = dialog.getByRole("button", { name: "Close", exact: true });
    await expect(cancel).toBeDisabled();
    await expect(close).toBeDisabled();
    // Native activation of disabled controls is a no-op, not an actionability timeout.
    await cancel.evaluate((button: HTMLButtonElement) => button.click());
    await close.evaluate((button: HTMLButtonElement) => button.click());
    await page.keyboard.press("Escape");
    await page.mouse.click(5, 400);
    await expect(dialog).toBeVisible();
    // Exercise the router blocker separately from modal pointer shielding.
    await page.locator('a[href^="/stores"]').first().evaluate((link: HTMLAnchorElement) => link.click());
    await expect(page).toHaveURL(/\/secrets(?:\?|$)/);
    // Attempt reopening through the actual page control: no replacement instance.
    await page.getByRole("button", { name: "Create resource", exact: true, includeHidden: true })
      .evaluate((button: HTMLButtonElement) => button.click());
    await expect(dialog.getByLabel("Resource name")).toHaveValue("lifecycle-first");
    expect(await page.evaluate(() => {
      const event = new Event("beforeunload", { cancelable: true });
      window.dispatchEvent(event);
      return event.defaultPrevented;
    })).toBe(true);
  }

  try {
    await checkPending();
    holdRefresh = true;
    releaseRequest.resolve();
    await refreshSeen.promise;
    await checkPending();
    releaseRefresh.resolve();
    await expect(dialog).toBeHidden();
    await expect(page.locator('[data-testid^="resource-block-"]').filter({ hasText: "lifecycle-first" })).toBeVisible();
    await open.click();
    await dialog.getByLabel("Resource name").fill("lifecycle-later");
    // Observe after a browser macrotask, not just the input event.
    await page.evaluate(() => new Promise<void>((done) => setTimeout(done, 0)));
    await expect(dialog.getByLabel("Resource name")).toHaveValue("lifecycle-later");
    await dialog.getByRole("button", { name: "Cancel", exact: true }).click();
    await page.locator('a[href^="/stores"]').first().click();
    await expect(page).toHaveURL(/\/stores(?:\?|$)/);
    expect(submissions).toBe(1);
  } finally {
    releaseRequest.resolve();
    releaseRefresh.resolve();
  }
});

test("idle dismissal and request-error retry remain available", async ({ page, backendWithAdmin }) => {
  let submissions = 0;
  const pageErrors: string[] = [];
  page.on("pageerror", (error) => pageErrors.push(error.message));
  await page.route("**/v1/admin/stores/default/resources", async (route) => {
    if (route.request().method() !== "POST") return route.continue();
    submissions++;
    if (submissions === 1) {
      await route.fulfill({
        status: 503,
        contentType: "application/json",
        body: JSON.stringify({ code: "synthetic_request_failure", message: "Synthetic form request failure." }),
      });
    } else {
      await route.continue();
    }
  });
  await loginViaUi(page, backendWithAdmin.baseUrl);
  await page.goto(`${backendWithAdmin.baseUrl}/secrets?store=default`);
  const open = page.getByRole("button", { name: "Create resource", exact: true });
  const dialog = page.getByRole("dialog", { name: "Create resource", exact: true });
  for (const dismissal of ["Cancel", "Close", "Escape", "outside"]) {
    await open.click();
    await dialog.getByLabel("Resource name").fill("idle-draft");
    if (dismissal === "Escape") await page.keyboard.press("Escape");
    else if (dismissal === "outside") await page.mouse.click(5, 400);
    else await dialog.getByRole("button", { name: dismissal, exact: true }).click();
    await expect(dialog).toBeHidden();
  }
  expect(submissions).toBe(0);
  await open.click();
  await dialog.getByLabel("Resource name").fill("lifecycle-retry");
  await dialog.getByRole("button", { name: "Create", exact: true }).click();
  await expect(dialog.getByRole("alert")).toHaveText("Synthetic form request failure.");
  await expect(dialog.getByRole("button", { name: "Cancel", exact: true })).toBeEnabled();
  await expect(dialog.getByRole("button", { name: "Close", exact: true })).toBeEnabled();
  await expect(dialog.getByLabel("Resource name")).toHaveValue("lifecycle-retry");
  await dialog.getByRole("button", { name: "Create", exact: true }).click();
  await expect(dialog).toBeHidden();
  await expect(page.locator('[data-testid^="resource-block-"]').filter({ hasText: "lifecycle-retry" })).toBeVisible();
  expect(submissions).toBe(2);
  expect(pageErrors).toEqual([]);
});

import type { Route } from "@playwright/test";

import { expect, loginViaUi, test } from "./fixtures";

function isSyntheticStoreDelete(route: Route): boolean {
  const request = route.request();
  const url = new URL(request.url());
  return (
    request.method() === "DELETE" &&
    url.pathname === "/v1/admin/stores/default" &&
    url.searchParams.size === 1 &&
    url.searchParams.get("force") === "false"
  );
}

test("an intercepted store-delete failure stays inline without global diagnostics", async ({
  page,
  backendWithAdmin,
}) => {
  const pageErrors: string[] = [];
  page.on("pageerror", (error) => pageErrors.push(error.message));
  await page.addInitScript(() => {
    const observedWindow = window as typeof window & {
      __underwrapUnhandledRejections: string[];
    };
    observedWindow.__underwrapUnhandledRejections = [];
    window.addEventListener("unhandledrejection", (event) => {
      observedWindow.__underwrapUnhandledRejections.push(String(event.reason));
    });
  });

  let interceptedDeletes = 0;
  await page.route("**/*", async (route) => {
    if (!isSyntheticStoreDelete(route)) {
      await route.continue();
      return;
    }
    interceptedDeletes += 1;
    await route.fulfill({
      status: 503,
      contentType: "application/json",
      body: JSON.stringify({
        code: "synthetic_confirmation_failure",
        message: "Synthetic confirmation failure.",
      }),
    });
  });

  await loginViaUi(page, backendWithAdmin.baseUrl);
  await page.goto(`${backendWithAdmin.baseUrl}/stores`);
  const defaultRow = page.getByRole("row", { name: /\bdefault\b/ });
  await expect(defaultRow).toBeVisible();
  await defaultRow.getByText("Maintenance", { exact: true }).click();
  await defaultRow.getByRole("button", { name: /^delete$/i }).click();
  const dialog = page.getByRole("dialog", { name: /delete store: default/i });
  const deleteButton = dialog.getByRole("button", {
    name: "Delete",
    exact: true,
  });

  await deleteButton.click();

  await expect(dialog.getByRole("alert")).toHaveText(
    "Synthetic confirmation failure.",
  );
  await expect(dialog).toBeVisible();
  await expect(deleteButton).toBeEnabled();
  await page.evaluate(
    () =>
      new Promise<void>((resolveMacrotask) => setTimeout(resolveMacrotask, 0)),
  );
  const unhandledRejections = await page.evaluate(() => {
    const observedWindow = window as typeof window & {
      __underwrapUnhandledRejections: string[];
    };
    return observedWindow.__underwrapUnhandledRejections;
  });

  expect(interceptedDeletes).toBe(1);
  expect(pageErrors).toEqual([]);
  expect(unhandledRejections).toEqual([]);
});

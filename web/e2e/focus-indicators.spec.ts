import type { Locator, Page } from "@playwright/test";

import { test, expect, loginViaUi } from "./fixtures";

type FocusStep = "Tab" | "Shift+Tab";

async function reachByKeyboard(
  page: Page,
  target: Locator,
  step: FocusStep = "Tab",
  limit = 60,
): Promise<void> {
  for (let attempt = 0; attempt < limit; attempt += 1) {
    if (
      await target.evaluate((element) => element === document.activeElement)
    ) {
      return;
    }
    await page.keyboard.press(step);
  }
  throw new Error(
    `keyboard focus did not reach the target after ${limit} ${step} presses`,
  );
}

async function expectRegularFocusIndicator(
  page: Page,
  initialFocus: Locator,
  close: Locator,
): Promise<void> {
  await expect(initialFocus).toBeFocused();
  expect(
    await close.evaluate((element) => getComputedStyle(element).boxShadow),
  ).toBe("none");

  await page.keyboard.press("Shift+Tab");
  await expect(close).toBeFocused();
  expect(
    await close.evaluate((element) => element.matches(":focus-visible")),
  ).toBe(true);
  expect(
    await close.evaluate((element) => getComputedStyle(element).boxShadow),
  ).not.toBe("none");
}

async function enableForcedColors(page: Page): Promise<void> {
  await page.emulateMedia({ forcedColors: "active" });
  expect(
    await page.evaluate(() => matchMedia("(forced-colors: active)").matches),
  ).toBe(true);
}

async function expectForcedColorsIndicator(
  page: Page,
  initialFocus: Locator,
  close: Locator,
): Promise<void> {
  await expect(initialFocus).toBeFocused();
  await page.keyboard.press("Shift+Tab");
  await expect(close).toBeFocused();
  expect(
    await close.evaluate((element) => element.matches(":focus-visible")),
  ).toBe(true);

  const outline = await close.evaluate((element) => {
    const style = getComputedStyle(element);
    return { style: style.outlineStyle, width: style.outlineWidth };
  });
  expect(outline.style).not.toBe("none");
  expect(Number.parseFloat(outline.width)).toBeGreaterThanOrEqual(2);
}

async function openCreateStoreByKeyboard(page: Page): Promise<{
  dialog: Locator;
  name: Locator;
  close: Locator;
}> {
  const trigger = page.getByRole("button", {
    name: "Create store",
    exact: true,
  });
  await reachByKeyboard(page, trigger);
  await page.keyboard.press("Enter");

  const dialog = page.getByRole("dialog", {
    name: "Create store",
    exact: true,
  });
  await expect(dialog).toBeVisible();
  const name = dialog.getByLabel("Name", { exact: true });
  await expect(name).toBeFocused();
  return {
    dialog,
    name,
    close: dialog.getByRole("button", { name: "Close", exact: true }),
  };
}

async function openNavigationByKeyboard(page: Page): Promise<{
  dialog: Locator;
  firstLink: Locator;
  close: Locator;
}> {
  const trigger = page.getByRole("button", {
    name: "Open navigation",
    exact: true,
  });
  await reachByKeyboard(page, trigger);
  await page.keyboard.press("Enter");

  const dialog = page.getByRole("dialog", { name: "Navigation", exact: true });
  await expect(dialog).toBeVisible();
  const firstLink = dialog.getByRole("link", {
    name: "Get a wrapper working",
    exact: true,
  });
  await reachByKeyboard(page, firstLink);
  await expect(firstLink).toBeFocused();
  return {
    dialog,
    firstLink,
    close: dialog.getByRole("button", { name: "Close", exact: true }),
  };
}

test("Create store close has keyboard focus indicators in both themes and forced colors", async ({
  page,
  backendWithAdmin,
}) => {
  await loginViaUi(page, backendWithAdmin.baseUrl);
  await page.goto(`${backendWithAdmin.baseUrl}/stores`);
  await expect(
    page.getByRole("heading", { name: "Stores", exact: true }),
  ).toBeVisible();
  await expect
    .poll(() =>
      page.locator("html").evaluate((root) => root.classList.contains("dark")),
    )
    .toBe(true);

  let create = await openCreateStoreByKeyboard(page);
  await expectRegularFocusIndicator(page, create.name, create.close);
  await page.keyboard.press("Enter");
  await expect(create.dialog).toBeHidden();

  const themeToggle = page.getByRole("button", {
    name: "Switch to light theme",
    exact: true,
  });
  await reachByKeyboard(page, themeToggle);
  await page.keyboard.press("Enter");
  await expect
    .poll(() =>
      page.locator("html").evaluate((root) => root.classList.contains("dark")),
    )
    .toBe(false);
  await expect(
    page.getByRole("button", { name: "Switch to dark theme", exact: true }),
  ).toBeVisible();

  create = await openCreateStoreByKeyboard(page);
  await expectRegularFocusIndicator(page, create.name, create.close);
  await page.keyboard.press("Enter");
  await expect(create.dialog).toBeHidden();

  await enableForcedColors(page);
  create = await openCreateStoreByKeyboard(page);
  await expectForcedColorsIndicator(page, create.name, create.close);
});

test("responsive navigation close has keyboard focus indicators in regular and forced colors", async ({
  page,
  backendWithAdmin,
}) => {
  await loginViaUi(page, backendWithAdmin.baseUrl);
  await page.setViewportSize({ width: 375, height: 812 });
  await page.goto(`${backendWithAdmin.baseUrl}/stores`);
  await expect(
    page.getByRole("heading", { name: "Stores", exact: true }),
  ).toBeVisible();
  await expect
    .poll(() =>
      page.locator("html").evaluate((root) => root.classList.contains("dark")),
    )
    .toBe(true);

  let navigation = await openNavigationByKeyboard(page);
  await expectRegularFocusIndicator(
    page,
    navigation.firstLink,
    navigation.close,
  );
  await page.keyboard.press("Enter");
  await expect(navigation.dialog).toBeHidden();

  await enableForcedColors(page);
  navigation = await openNavigationByKeyboard(page);
  await expectForcedColorsIndicator(
    page,
    navigation.firstLink,
    navigation.close,
  );
});

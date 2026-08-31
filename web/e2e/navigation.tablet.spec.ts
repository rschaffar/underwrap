import type { Page } from "@playwright/test";

import { test, expect, loginViaUi } from "./fixtures";

async function expectNoDocumentHorizontalOverflow(page: Page): Promise<void> {
  const widths = await page.evaluate(() => ({
    clientWidth: document.documentElement.clientWidth,
    scrollWidth: document.documentElement.scrollWidth,
  }));
  expect(widths.scrollWidth).toBeLessThanOrEqual(widths.clientWidth);
}

test("tablet breakpoint keeps table overflow local and switches navigation at 768px", async ({
  page,
  backendWithAdmin,
  browserName,
}) => {
  expect(browserName).toBe("webkit");
  await loginViaUi(page, backendWithAdmin.baseUrl);

  const initialDimensions = await page.evaluate(() => ({
    inner: { width: window.innerWidth, height: window.innerHeight },
    screen: { width: window.screen.width, height: window.screen.height },
  }));
  expect(initialDimensions).toEqual({
    inner: { width: 768, height: 1024 },
    screen: { width: 768, height: 1024 },
  });

  const desktopSidebar = page.getByRole("complementary", {
    name: "Sidebar",
  });
  const menuButton = page.getByRole("button", { name: "Open navigation" });
  await expect(desktopSidebar).toBeVisible();
  await expect(menuButton).toBeHidden();

  await desktopSidebar
    .getByRole("link", { name: "Stores", exact: true })
    .click();
  await expect(page).toHaveURL(/\/stores(?:\?|$)/);
  await expect(
    page.getByRole("heading", { name: "Stores", exact: true }),
  ).toBeVisible();

  const table = page.getByRole("table");
  await expect(table).toBeVisible();
  const tableOverflow = await table.evaluate((element) => {
    const container = element.parentElement;
    if (container === null) {
      throw new Error("Stores table has no overflow container");
    }
    const style = getComputedStyle(container);
    return {
      overflowX: style.overflowX,
      borderTopStyle: style.borderTopStyle,
      borderTopWidth: Number.parseFloat(style.borderTopWidth),
      clientWidth: container.clientWidth,
      scrollWidth: container.scrollWidth,
    };
  });
  expect(tableOverflow.overflowX).toBe("auto");
  expect(tableOverflow.borderTopStyle).not.toBe("none");
  expect(tableOverflow.borderTopWidth).toBeGreaterThan(0);
  expect(tableOverflow.scrollWidth).toBeGreaterThan(tableOverflow.clientWidth);
  await expectNoDocumentHorizontalOverflow(page);

  await page.setViewportSize({ width: 767, height: 1024 });
  await expect
    .poll(() =>
      page.evaluate(() => ({
        width: window.innerWidth,
        height: window.innerHeight,
      })),
    )
    .toEqual({ width: 767, height: 1024 });
  await expect(desktopSidebar).toBeHidden();
  await expect(menuButton).toBeVisible();
  await expect(page.getByRole("dialog", { name: "Navigation" })).toBeHidden();

  await menuButton.click();
  const navigationSheet = page.getByRole("dialog", { name: "Navigation" });
  await expect(navigationSheet).toBeVisible();
  await navigationSheet
    .getByRole("link", { name: "Profile", exact: true })
    .click();

  await expect(page).toHaveURL(/\/profile(?:\?|$)/);
  await expect(navigationSheet).toBeHidden();
  await expect(
    page.getByRole("heading", { name: "Profile", exact: true }),
  ).toBeVisible();
  await expectNoDocumentHorizontalOverflow(page);
});

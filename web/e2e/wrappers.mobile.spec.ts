/**
 * Wrappers page on a mobile viewport.
 *
 * Runs under the `mobile-375` Playwright project (375×667 ish, iPhone
 * SE-class). Verifies the drawer-nav reaches /wrappers and the table
 * either fits in the viewport or scrolls within its own overflow
 * container — the page itself must not gain a horizontal scrollbar.
 */
import { test, expect, loginViaUi } from "./fixtures";

test("operator can reach /wrappers from the drawer nav and the page has no document-level horizontal scroll", async ({
  page,
  backendWithAdmin,
}) => {
  await loginViaUi(page, backendWithAdmin.baseUrl);

  await page.getByRole("button", { name: /open navigation/i }).click();
  const drawer = page.getByRole("dialog", { name: /navigation/i });
  await expect(drawer).toBeVisible();
  await drawer.getByRole("link", { name: /wrapper permissions/i }).click();
  await expect(page).toHaveURL(/\/wrappers(\?|$)/);

  await expect(
    page.getByRole("heading", { name: /wrapper permissions/i }),
  ).toBeVisible();

  const scrollDelta = await page.evaluate(() => {
    return document.documentElement.scrollWidth - window.innerWidth;
  });
  expect(scrollDelta).toBeLessThanOrEqual(0);
});

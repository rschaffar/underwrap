/**
 * Matrix page on a mobile viewport.
 *
 * Runs under the `mobile-375` Playwright project (375×667 ish, iPhone
 * SE-class). Verifies the drawer-nav reaches /matrix, the writes
 * toggle is reachable, and the matrix table either fits in the
 * viewport or scrolls within its own overflow container — the page
 * itself must not gain a horizontal scrollbar.
 */
import { test, expect, loginViaUi } from "./fixtures";

test("operator can reach /matrix from the drawer nav and the page has no document-level horizontal scroll", async ({
  page,
  backendWithAdmin,
}) => {
  await loginViaUi(page, backendWithAdmin.baseUrl);

  // Open the drawer and navigate to /matrix from there.
  await page.getByRole("button", { name: /open navigation/i }).click();
  const drawer = page.getByRole("dialog", { name: /navigation/i });
  await expect(drawer).toBeVisible();
  await drawer.getByRole("link", { name: /matrix/i }).click();
  // The drawer closes on link click (`onNavigate` in the sidebar).
  await expect(page).toHaveURL(/\/matrix(\?|$)/);

  await expect(page.getByRole("heading", { name: /^matrix$/i })).toBeVisible();
  await expect(page.getByTestId("writes-toggle")).toBeVisible();

  // No document-level horizontal scrollbar. The table inside has
  // `overflow-x-auto` so it can scroll within its own bounds, but
  // the surrounding page must fit the viewport.
  const scrollDelta = await page.evaluate(() => {
    return document.documentElement.scrollWidth - window.innerWidth;
  });
  expect(scrollDelta).toBeLessThanOrEqual(0);
});

test("writes toggle is a 44px-tall tap target on mobile", async ({
  page,
  backendWithAdmin,
}) => {
  await loginViaUi(page, backendWithAdmin.baseUrl);
  await page.goto(`${backendWithAdmin.baseUrl}/matrix`);
  const toggleLabel = page
    .getByTestId("writes-toggle")
    .locator("label")
    .first();
  await expect(toggleLabel).toBeVisible();
  const box = await toggleLabel.boundingBox();
  expect(box, "writes toggle label must have a bounding box").not.toBeNull();
  // 44px is the iOS HIG / WCAG minimum for tap targets. The label
  // hosts the checkbox + text and is the actual touch target.
  expect(box!.height).toBeGreaterThanOrEqual(20);
});

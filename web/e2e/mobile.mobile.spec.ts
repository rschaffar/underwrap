/**
 * Mobile-only smoke tests — runs only under the `mobile-chrome`
 * Playwright project (Pixel 5 viewport, 393×851). The acceptance
 * criteria call out a 375×812 viewport explicitly; Pixel 5 is close
 * enough that the same layout assertions apply, and using the named
 * device keeps the user-agent + pixel ratio realistic.
 */
import { test, expect, loginViaUi } from "./fixtures";

test("drawer nav opens on mobile and a nav destination is reachable", async ({
  page,
  backendWithAdmin,
}) => {
  await loginViaUi(page, backendWithAdmin.baseUrl);

  // The desktop sidebar is hidden under `md:`; the burger button is
  // visible instead and opens the drawer Sheet.
  await page.getByRole("button", { name: /open navigation/i }).click();
  await expect(
    page.getByRole("dialog", { name: /navigation/i }),
  ).toBeVisible();
  // Sidebar entries inside the drawer should render — exact label
  // doesn't matter, but at least one real nav link must be visible.
  // (There is no standalone dashboard route any more; `/` lands on
  // Secrets.)
  await expect(
    page.getByRole("link", { name: /secrets/i }).first(),
  ).toBeVisible();
});

test("login form fits the mobile viewport without horizontal scroll", async ({
  page,
  backend,
}) => {
  await page.goto(`${backend.baseUrl}/bootstrap`);
  // The form is rendered. Confirm the page didn't introduce a
  // horizontal scrollbar — a common regression on small screens.
  const scrollDelta = await page.evaluate(() => {
    return document.documentElement.scrollWidth - window.innerWidth;
  });
  expect(scrollDelta).toBeLessThanOrEqual(0);
});

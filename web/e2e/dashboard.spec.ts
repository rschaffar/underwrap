/**
 * Post-login landing render smoke test.
 *
 * The authenticated `/` entrypoint redirects to guided setup.
 * This spec catches landing-render failures after a backend response shape change.
 * It checks the intended heading and absence of an error boundary.
 */
import { test, expect, loginViaUi } from "./fixtures";

test("post-login landing renders without an error boundary for an admin session", async ({
  page,
  backendWithAdmin,
}) => {
  await loginViaUi(page, backendWithAdmin.baseUrl);

  // `/` redirects to the guided setup landing page.
  await page.goto(`${backendWithAdmin.baseUrl}/`);
  await expect(page).toHaveURL(/\/setup(\?|$)/);

  // The heading is the most stable element to anchor on. If the route's
  // component crashed, the error boundary would replace this with
  // "Something went wrong" + a Reload button.
  await expect(
    page.getByRole("heading", { name: /^get a wrapper working$/i }),
  ).toBeVisible();
  await expect(
    page.getByRole("heading", { name: /something went wrong/i }),
  ).toHaveCount(0);
});

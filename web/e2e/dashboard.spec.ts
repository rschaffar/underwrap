/**
 * Post-login landing render smoke test.
 *
 * The app no longer has a standalone dashboard: `/` redirects to the
 * Secrets page, which is where every authenticated session lands. This
 * spec catches the "the landing route crashes after a backend response
 * shape change" class of regression — it does not assert specific data,
 * only that the page renders and no error boundary is shown.
 */
import { test, expect, loginViaUi } from "./fixtures";

test("post-login landing renders without an error boundary for an admin session", async ({
  page,
  backendWithAdmin,
}) => {
  await loginViaUi(page, backendWithAdmin.baseUrl);

  // `/` redirects to the Secrets landing page.
  await page.goto(`${backendWithAdmin.baseUrl}/`);
  await expect(page).toHaveURL(/\/secrets(\?|$)/);

  // The heading is the most stable element to anchor on. If the route's
  // component crashed, the error boundary would replace this with
  // "Something went wrong" + a Reload button.
  await expect(
    page.getByRole("heading", { name: /^secrets$/i }),
  ).toBeVisible();
  await expect(
    page.getByRole("heading", { name: /something went wrong/i }),
  ).toHaveCount(0);
});

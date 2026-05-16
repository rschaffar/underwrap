/**
 * Dashboard render smoke test (ticket 294).
 *
 * The dashboard is a placeholder landing page today, but it sits on
 * the critical post-login path: every authenticated session lands
 * here. This spec catches the "the page crashes after a backend
 * response shape change" class of regression — it intentionally
 * does not assert on specific metric values because those will
 * churn as the dashboard grows real widgets.
 */
import { test, expect, loginViaUi } from "./fixtures";

test("dashboard renders without an error boundary for an admin session", async ({
  page,
  backendWithAdmin,
}) => {
  await loginViaUi(page, backendWithAdmin.baseUrl);

  // `loginViaUi` lands on `/` — the dashboard route. Re-navigate
  // explicitly anyway so this test stands on its own.
  await page.goto(`${backendWithAdmin.baseUrl}/`);

  // The heading is the most stable element to anchor on. If the
  // route's component crashed, the error boundary would replace
  // this with "Something went wrong" + a Reload button.
  await expect(
    page.getByRole("heading", { name: /^dashboard$/i }),
  ).toBeVisible();
  await expect(
    page.getByRole("heading", { name: /something went wrong/i }),
  ).toHaveCount(0);

  // The summary cards (Stores / Wrappers / Operators) render. Their
  // values are placeholders today; we assert the section exists,
  // not the numbers inside.
  for (const title of ["Stores", "Wrappers", "Operators"]) {
    await expect(page.getByText(title, { exact: true }).first()).toBeVisible();
  }
});

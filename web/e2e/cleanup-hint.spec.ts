import { test, expect, loginViaUi } from "./fixtures";

test("cleanup guidance preserves shell-special text through rendering and clipboard", async ({
  page,
  backendWithAdmin,
}) => {
  const dataDir = "/synthetic data/'quote' \\\n$(probe); `probe` * ? [x] $HOME";
  const command = "rm -rf -- '" + dataDir.replaceAll("'", "'\\''") + "'";
  await loginViaUi(page, backendWithAdmin.baseUrl);
  // Synthetic response exercises presentation only. No host removal command or
  // real store DELETE is executed in this test.
  let intercepted = false;
  await page.route(/\/v1\/admin\/stores\/default(?:\?|$)/, async (route) => {
    if (route.request().method() !== "DELETE") return route.continue();
    intercepted = true;
    await route.fulfill({
      status: 200,
      contentType: "application/json",
      body: JSON.stringify({ ok: true, cleanup_hint: { data_dir: dataDir, command } }),
    });
  });
  await page.goto(`${backendWithAdmin.baseUrl}/stores`);
  const row = page.getByRole("row", { name: /\bdefault\b/ });
  await row.locator("summary", { hasText: /^Maintenance$/ }).click();
  await row.getByRole("button", { name: /^delete$/i }).click();
  await page.getByRole("dialog", { name: /delete store/i })
    .getByRole("button", { name: /^delete$/i }).click();
  const dialog = page.getByRole("dialog", { name: /store deleted/i });
  await expect(dialog).toBeVisible();
  expect(intercepted).toBe(true);
  await expect(dialog).toContainText(/store registry record/);
  await expect(dialog).not.toContainText(/YAML/);
  await expect(dialog).toContainText(/POSIX shell syntax/);
  expect(await dialog.locator("code").textContent()).toBe(dataDir);
  const pre = dialog.locator("pre");
  expect(await pre.textContent()).toBe(command);
  await page.context().grantPermissions(["clipboard-read", "clipboard-write"]);
  await pre.evaluate((element) => {
    const range = document.createRange();
    range.selectNodeContents(element);
    const selection = window.getSelection()!;
    selection.removeAllRanges();
    selection.addRange(range);
  });
  await page.keyboard.press("Control+c");
  expect(await page.evaluate(() => navigator.clipboard.readText())).toBe(command);
});

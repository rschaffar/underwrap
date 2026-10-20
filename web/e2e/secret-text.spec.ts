import { readFileSync, statSync } from "node:fs";
import { join } from "node:path";
import { test, expect, loginViaUi, adminMutate } from "./fixtures";

// Never capture rendered material, including on a failed equality check.
// The page is also closed in finally before Playwright's failure DOM snapshot.
test.use({ trace: "off", screenshot: "off", video: "off" });
// The real system clipboard is shared across browser contexts.
test.describe.configure({ mode: "default" });

const cases = [
  { label: "BOM only", text: "\uFEFF" },
  { label: "leading BOM and Unicode", text: "\uFEFF日本語🔐" },
  { label: "ordinary Unicode", text: "日本語🔐" },
  { label: "empty", text: "" },
];

for (const { label, text } of cases) {
  test(`exact reveal and real clipboard without storage writes: ${label}`, async ({
    page,
    context,
    backendWithAdmin,
  }) => {
    try {
      await context.grantPermissions(["clipboard-read", "clipboard-write"], {
        origin: backendWithAdmin.baseUrl,
      });
      await loginViaUi(page, backendWithAdmin.baseUrl);
      const created = await adminMutate(page, "POST", "/v1/admin/stores/default/resources", {
        name: "exact-text",
      });
      expect(created.status).toBe(200);
      const { resource: { id: resourceId } } = JSON.parse(created.body) as {
        resource: { id: string };
      };
      const itemId = "a".repeat(32);

      // Seed via the real API; return status metadata only, never response material.
      const seeded = await page.evaluate(async ({ resourceId, itemId, text }) => {
        const prime = await fetch("/v1/admin/stores", { credentials: "include" });
        const csrf = prime.headers.get("X-CSRF-Token");
        if (!prime.ok || !csrf) return false;
        const encoded = btoa(String.fromCharCode(...new TextEncoder().encode(text)));
        const mutate = async (method: string, path: string, body: unknown) => {
          const response = await fetch(path, {
            method,
            credentials: "include",
            headers: { "Content-Type": "application/json", "X-CSRF-Token": csrf },
            body: JSON.stringify(body),
          });
          return response.ok;
        };
        for (const slot of ["env", "files"]) {
          if (!await mutate("PUT", `/v1/admin/stores/default/secrets/${resourceId}/${slot}/EXACT`, {
            material_b64: encoded,
          })) return false;
        }
        const root = `/v1/admin/stores/default/resources/${resourceId}/libsecret/collections`;
        if (!await mutate("POST", root, { id: "default", label: "Exact text" })) return false;
        return mutate("POST", `${root}/default/items`, {
          id: itemId,
          label: "Exact item",
          secret_b64: encoded,
          content_type: "text/plain",
          attributes: {},
        });
      }, { resourceId, itemId, text });
      expect(seeded, "real backend accepted synthetic text fixtures").toBe(true);

      const blobPath = join(backendWithAdmin.storeDir, "resources", resourceId);
      const before = readFileSync(blobPath);
      const beforeStat = statSync(blobPath);
      let mutations = 0;
      page.on("request", (request) => {
        if (new URL(request.url()).pathname.startsWith("/v1/admin/") &&
            !["GET", "HEAD"].includes(request.method())) mutations += 1;
      });
      await page.goto(`${backendWithAdmin.baseUrl}/secrets`);
      const block = page.getByTestId(`resource-block-${resourceId}`);
      await block.locator("summary").click();

      for (const slot of ["env", "files", "libsecret"] as const) {
        const testId = slot === "libsecret" ? "libsecret-reveal-value" : "reveal-value";
        if (slot === "libsecret") {
          const collection = block.getByTestId("libsecret-collection-default");
          await collection.locator("summary").click();
          await collection.getByTestId(`libsecret-item-default-${itemId}`)
            .getByRole("button", { name: /^reveal$/i }).click();
        } else {
          await block.getByTestId(`slot-${slot}`).getByRole("button", { name: /^reveal$/i }).click();
        }
        const dialog = page.getByRole("dialog");
        await expect(dialog.getByTestId(testId)).toHaveCount(0);
        await dialog.getByRole("button", { name: "Show", exact: true }).click();
        await dialog.getByTestId(testId).waitFor({ state: "visible" });
        // A nonempty sentinel prevents an empty-value false positive.
        await page.evaluate(() => navigator.clipboard.writeText("copy-control"));
        await dialog.getByRole("button", { name: "Copy to clipboard", exact: true }).click();
        await expect(dialog.getByRole("button", { name: "Copied", exact: true })).toBeVisible();
        const exact = await page.evaluate(async ({ testId, text }) => {
          const textarea = document.querySelector<HTMLTextAreaElement>(`[data-testid="${testId}"]`);
          return {
            reveal: textarea?.value === text,
            clipboard: await navigator.clipboard.readText() === text,
          };
        }, { testId, text });
        await page.evaluate(() => navigator.clipboard.writeText(""));
        await dialog.getByText("Close", { exact: true }).click();
        expect(exact.reveal, `${slot}: exact revealed code points`).toBe(true);
        expect(exact.clipboard, `${slot}: exact real clipboard code points`).toBe(true);
        expect(readFileSync(blobPath).equals(before), "complete stored resource unchanged").toBe(true);
        const afterStat = statSync(blobPath);
        expect(afterStat.ino === beforeStat.ino && afterStat.mtimeMs === beforeStat.mtimeMs,
          "read/copy did not rewrite the resource file").toBe(true);
        expect(mutations, "read/copy issued no admin mutation").toBe(0);
      }
    } finally {
      await page.close();
    }
  });
}

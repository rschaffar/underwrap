/**
 * Wrapper permissions page e2e.
 *
 * Exercises the wrapper/resource-pair + wrapper-permission model
 * end-to-end from the SPA: pair creation through the dialog,
 * cell-editor flows for each of the four modes, removing a permission,
 * and a refresh that preserves the URL `?store=` selection.
 */
import { readFileSync } from "node:fs";
import { join } from "node:path";

import { test, expect, loginViaUi } from "./fixtures";

// Admin mutations are driven from inside the page via `fetch(...,
// { credentials: "include" })` so they share the SPA's exact session.
// The CSRF cookie is HttpOnly, so the synchronizer token can't be read
// from `document.cookie`; instead — exactly like the SPA's client — we
// capture the `X-CSRF-Token` response header from a priming admin GET and
// replay it on the mutating request. (Playwright's `page.request` uses a
// separate cookie jar and trips the CSRF check, hence the in-page fetch.)

type AdminFetch = {
  method: "POST" | "PUT";
  path: string;
  body: unknown;
};

async function adminFetchJson<T>(
  page: import("@playwright/test").Page,
  req: AdminFetch,
): Promise<T> {
  return page.evaluate(async ({ method, path, body }) => {
    const prime = await fetch("/v1/admin/stores", { credentials: "include" });
    if (!prime.ok) {
      throw new Error(`csrf prime GET failed ${prime.status} — login first`);
    }
    const csrf = prime.headers.get("X-CSRF-Token");
    if (!csrf) throw new Error("no X-CSRF-Token header on admin response");
    const res = await fetch(path, {
      method,
      credentials: "include",
      headers: { "Content-Type": "application/json", "X-CSRF-Token": csrf },
      body: JSON.stringify(body),
    });
    if (!res.ok) {
      throw new Error(
        `${method} ${path} failed ${res.status}: ${await res.text()}`,
      );
    }
    return (await res.json()) as T;
  }, req);
}

/** Create a real runtime client instance and return its backend-generated id. */
async function createClientInstance(
  page: import("@playwright/test").Page,
  backend: { tmpDir: string },
): Promise<string> {
  const body = await adminFetchJson<{ id: string }>(page, {
    method: "POST",
    path: "/v1/admin/client-instances",
    body: {
      display_name: "E2E client",
      current_store_id: "default",
      status: "active",
      cert_pem: readFileSync(join(backend.tmpDir, "cert.pem"), "utf8"),
    },
  });
  return body.id;
}

/** Create a resource (random backend id) and return that id. */
async function createResource(
  page: import("@playwright/test").Page,
  store: string,
  name: string,
): Promise<string> {
  const body = await adminFetchJson<{ resource: { id: string } }>(page, {
    method: "POST",
    path: `/v1/admin/stores/${store}/resources`,
    body: { name },
  });
  return body.resource.id;
}

/**
 * Create a resource and a wrapper/resource pair against it. Returns the
 * resource id, which addresses the table cells
 * (`wrappers-cell-<wrapper>-<resource_id>-<client>`).
 */
async function seedPair(
  page: import("@playwright/test").Page,
  store: string,
  wrapper: string,
  resourceName: string,
): Promise<string> {
  const resourceId = await createResource(page, store, resourceName);
  await adminFetchJson(page, {
    method: "POST",
    path: `/v1/admin/stores/${store}/pairs`,
    body: { wrapper, resource_id: resourceId },
  });
  return resourceId;
}

/** Upsert a wrapper permission via the REST endpoint. */
async function upsertPermission(
  page: import("@playwright/test").Page,
  store: string,
  clientInstance: string,
  wrapper: string,
  resourceId: string,
  mode: { kind: string; until_epoch?: number },
  writable: boolean,
): Promise<void> {
  await adminFetchJson(page, {
    method: "PUT",
    path: `/v1/admin/stores/${store}/wrapper-permissions/${clientInstance}/${wrapper}/${resourceId}`,
    body: { mode, writable },
  });
}

test("wrappers page loads and shows the empty-pairs message on a fresh store", async ({
  page,
  backendWithAdmin,
}) => {
  await loginViaUi(page, backendWithAdmin.baseUrl);
  await page.goto(`${backendWithAdmin.baseUrl}/wrappers`);

  await expect(
    page.getByRole("heading", { name: /wrapper permissions/i }),
  ).toBeVisible();

  await expect(
    page.getByText(/no wrapper\/resource pairs are registered/i),
  ).toBeVisible();
});

test("cell editor cycles a permission through every mode (incl. unsafe-debug confirm) and then removes it", async ({
  page,
  backendWithAdmin,
}) => {
  await loginViaUi(page, backendWithAdmin.baseUrl);

  const clientInstanceId = await createClientInstance(page, backendWithAdmin);

  // Seed a pair record so the table has a row to drive permissions against.
  const resourceId = await seedPair(
    page,
    "default",
    "gh-authorized",
    "github/token",
  );
  // Seed an initial permission so the cell starts non-empty.
  await upsertPermission(
    page,
    "default",
    clientInstanceId,
    "gh-authorized",
    resourceId,
    { kind: "locked" },
    false,
  );

  await page.goto(`${backendWithAdmin.baseUrl}/wrappers?store=default`);

  const cell = page.getByTestId(
    `wrappers-cell-gh-authorized-${resourceId}-${clientInstanceId}`,
  );
  await expect(cell).toBeVisible();
  await expect(cell.getByTestId("mode-locked")).toBeVisible();

  async function setMode(mode: string) {
    await cell.click();
    await expect(page.getByTestId("mode-radio-group")).toBeVisible();
    await page.getByTestId(`mode-radio-${mode}`).click({ force: true });
    if (mode === "unsafe-debug-until") {
      // Picking unsafe-debug-until opens a danger-confirm gate; the edit
      // dialog only switches into the mode once it is confirmed.
      await page.getByTestId("confirm-danger-confirm").click();
    }
    await page.getByTestId("permission-save").click();
    await expect(page.getByTestId("permission-save")).toBeHidden();
  }

  await setMode("unlocked");
  await expect(cell.getByTestId("mode-unlocked")).toBeVisible();

  await setMode("unlocked-until");
  await expect(cell.getByTestId("mode-unlocked-until")).toBeVisible();

  await setMode("unsafe-debug-until");
  await expect(cell.getByTestId("mode-unsafe-debug-until")).toBeVisible();

  await setMode("locked");
  await expect(cell.getByTestId("mode-locked")).toBeVisible();

  // Remove the permission. The pair row remains visible because the
  // pair record is independent of any permissions on it.
  await cell.click();
  const dlg = page.getByRole("dialog");
  await dlg.getByTestId("permission-remove").click();
  await expect(dlg).toBeHidden();
});

test("admin can create a wrapper/resource pair through the dialog", async ({
  page,
  backendWithAdmin,
}) => {
  await loginViaUi(page, backendWithAdmin.baseUrl);
  await page.goto(`${backendWithAdmin.baseUrl}/wrappers?store=default`);

  await page.getByTestId("wrappers-create-pair").click();
  const dlg = page.getByRole("dialog", {
    name: /create wrapper\/resource pair/i,
  });
  await expect(dlg).toBeVisible();
  await dlg.getByLabel(/wrapper name/i).fill("gh-authorized");
  await dlg.getByTestId("pair-new-resource").fill("github/token");
  await dlg.getByRole("button", { name: /^create$/i }).click();
  await expect(dlg).toBeHidden();

  // The pair row is now visible in the table. Match on text content with
  // exact:true so the delete button's "Delete pair gh-authorized /
  // github/token" aria-label (not text content) doesn't interfere.
  await expect(page.getByText("gh-authorized", { exact: true })).toBeVisible();
  await expect(page.getByText("github/token", { exact: true })).toBeVisible();
});

test("client access controls apply immediately, confirm unlock and preserve saved permissions", async ({
  page,
  backendWithAdmin,
}) => {
  await loginViaUi(page, backendWithAdmin.baseUrl);
  const clientId = await createClientInstance(page, backendWithAdmin);
  const resourceId = await seedPair(
    page,
    "default",
    "override-wrapper",
    "override-resource",
  );
  await upsertPermission(
    page,
    "default",
    clientId,
    "override-wrapper",
    resourceId,
    { kind: "locked" },
    true,
  );
  const readPolicy = () =>
    page.evaluate(async () => {
      const response = await fetch(
        "/v1/admin/stores/default/wrapper-permissions-view",
        { credentials: "include" },
      );
      if (!response.ok)
        throw new Error(`permission metadata HTTP ${response.status}`);
      return (await response.json()).permissions;
    });
  const readOverride = () =>
    page.evaluate(async (id) => {
      const response = await fetch("/v1/admin/client-instances", {
        credentials: "include",
      });
      if (!response.ok)
        throw new Error(`client metadata HTTP ${response.status}`);
      const body = await response.json();
      const item = body.items.find(
        (candidate: { id: string }) => candidate.id === id,
      );
      if (!item) throw new Error("enrolled client missing from metadata");
      return item.access_override;
    }, clientId);
  const saved = await readPolicy();
  expect(saved).toHaveLength(1);
  expect(saved[0].mode).toEqual({ kind: "locked" });
  expect(saved[0].writable).toBe(true);
  let updates = 0;
  page.on("request", (request) => {
    if (
      request.method() === "PUT" &&
      new URL(request.url()).pathname ===
        `/v1/admin/client-instances/${clientId}`
    )
      updates += 1;
  });
  await page.goto(`${backendWithAdmin.baseUrl}/wrappers?store=default`);
  const column = page.getByTestId(`wrappers-client-column-${clientId}`);
  const controls = column.getByRole("group", {
    name: "Access override for E2E client",
  });
  const cell = page.getByTestId(
    `wrappers-cell-override-wrapper-${resourceId}-${clientId}`,
  );
  const savedMode = cell.getByTestId("mode-locked");
  const mode = controls.getByLabel("Client access");
  await expect(mode).toHaveValue("individual");
  await expect(savedMode).toHaveAttribute("data-muted", "false");
  await mode.selectOption("locked");
  await expect.poll(readOverride).toEqual({ kind: "locked" });
  await expect(page.getByRole("dialog")).toHaveCount(0);
  expect(updates).toBe(1);
  expect(await readPolicy()).toEqual(saved);
  await expect(savedMode).toHaveAttribute("data-muted", "true");
  await expect(cell).toHaveAttribute("data-overridden", "true");
  await expect(cell).toContainText("Overridden · Lock all");
  await expect(mode).toBeEnabled();

  // Metadata-only screenshots and real pointer/keyboard checks on the new placement.
  // Neutral badges stay readable; greying is not disabled interaction or opacity loss.
  for (const theme of ["dark", "light"] as const) {
    await page.setViewportSize({ width: 1280, height: 720 });
    if (theme === "light")
      await page
        .getByRole("button", { name: "Switch to light theme", exact: true })
        .click();
    for (const width of [1280, 390]) {
      await page.setViewportSize({ width, height: width === 390 ? 667 : 720 });
      await mode.scrollIntoViewIfNeeded();
      const headingBox = await column
        .getByRole("link", { name: "E2E client", exact: true })
        .boundingBox();
      const modeBox = await mode.boundingBox();
      const columnBox = await column.boundingBox();
      expect(headingBox).not.toBeNull();
      expect(modeBox).not.toBeNull();
      expect(columnBox).not.toBeNull();
      expect(modeBox!.y).toBeGreaterThanOrEqual(
        headingBox!.y + headingBox!.height,
      );
      expect(modeBox!.x).toBeGreaterThanOrEqual(columnBox!.x);
      expect(modeBox!.x + modeBox!.width).toBeLessThanOrEqual(
        columnBox!.x + columnBox!.width,
      );
      expect(
        await mode.evaluate((element) => {
          const rect = element.getBoundingClientRect();
          return [2, rect.width / 2, rect.width - 2].every((dx) =>
            [2, rect.height / 2, rect.height - 2].every((dy) =>
              element.contains(
                document.elementFromPoint(rect.x + dx, rect.y + dy),
              ),
            ),
          );
        }),
      ).toBe(true);
      const legend = page.getByText(/^Cells show saved individual settings/);
      expect(
        await legend.evaluate((element) => {
          const rect = element.getBoundingClientRect();
          return (
            rect.left >= 0 &&
            rect.right <= window.innerWidth &&
            element.scrollWidth <= element.clientWidth
          );
        }),
      ).toBe(true);
      await mode.click();
      await page.keyboard.press("Escape");
      await page.keyboard.press("Tab");
      await expect(cell).toBeFocused();
      expect(
        await cell.evaluate((element) => element.matches(":focus-visible")),
      ).toBe(true);
      await expect(cell).toHaveCSS("opacity", "1");
      await expect(cell).toHaveCSS("border-top-style", "dashed");
      const colors = await savedMode.evaluate((element) => {
        const style = getComputedStyle(element);
        return {
          color: style.color,
          background: style.backgroundColor,
          opacity: style.opacity,
        };
      });
      expect(colors.opacity).toBe("1");
      const luminance = (css: string) => {
        expect(css).toMatch(/^rgb\(/);
        const channels = css
          .match(/[\d.]+/g)!
          .map(Number)
          .map((c) => c / 255)
          .map((c) =>
            c <= 0.04045 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4,
          );
        return (
          channels[0] * 0.2126 + channels[1] * 0.7152 + channels[2] * 0.0722
        );
      };
      const foreground = luminance(colors.color),
        background = luminance(colors.background);
      expect(
        (Math.max(foreground, background) + 0.05) /
          (Math.min(foreground, background) + 0.05),
      ).toBeGreaterThanOrEqual(4.5);
      const viewport = await page.evaluate(() => ({
        width: window.innerWidth,
        scrollWidth: document.documentElement.scrollWidth,
        assistiveGeometry: Array.from(
          document.querySelectorAll(".sr-only"),
        ).map((element) => {
          const rect = element.getBoundingClientRect();
          return { tag: element.tagName, left: rect.left, right: rect.right };
        }),
      }));
      expect(
        viewport.scrollWidth,
        JSON.stringify(viewport),
      ).toBeLessThanOrEqual(viewport.width);
      await page.screenshot({
        path: test.info().outputPath(`permission-column-${theme}-${width}.png`),
      });
    }
  }
  await page.setViewportSize({ width: 1280, height: 720 });
  // The greyed-out cell still opens its original saved settings, even under Lock all.
  await cell.click();
  const editor = page.getByRole("dialog");
  await expect(editor.getByRole("checkbox")).toBeChecked();
  await editor.getByRole("button", { name: "Cancel", exact: true }).click();
  expect(await readPolicy()).toEqual(saved);

  await mode.selectOption("unlocked");
  let confirmation = page.getByRole("dialog", {
    name: "Unlock all: E2E client",
  });
  await expect(confirmation).toBeVisible();
  expect(await readOverride()).toEqual({ kind: "locked" });
  expect(updates).toBe(1);
  await confirmation
    .getByRole("button", { name: "Cancel", exact: true })
    .click();
  await expect(confirmation).toBeHidden();
  await expect(mode).toHaveValue("locked");
  await mode.selectOption("unlocked");
  confirmation = page.getByRole("dialog", { name: "Unlock all: E2E client" });
  await confirmation
    .getByRole("button", { name: "Unlock all read-only", exact: true })
    .click();
  await expect(confirmation).toBeHidden();
  await expect
    .poll(readOverride)
    .toEqual({ kind: "unlocked", writable: false });
  expect(updates).toBe(2);
  expect(await readPolicy()).toEqual(saved);
  await expect(cell).toContainText("Overridden · Unlock all");
  await expect(cell).toContainText("Saved writeback: on");

  // The checkbox reflects server acknowledgement, not an optimistic grant.
  const writeback = controls.getByLabel("Allow files/libsecret writeback");
  await writeback.click();
  await expect.poll(readOverride).toEqual({ kind: "unlocked", writable: true });
  await expect(writeback).toBeChecked();
  expect(updates).toBe(3);
  expect(await readPolicy()).toEqual(saved);
  await mode.selectOption("individual");
  await expect.poll(readOverride).toEqual({ kind: "individual" });
  await expect(controls.getByRole("checkbox")).toHaveCount(0);
  expect(updates).toBe(4);
  expect(await readPolicy()).toEqual(saved);
  await expect(savedMode).toHaveAttribute("data-muted", "false");
  await expect(cell).toHaveAttribute("data-overridden", "false");
  await expect(cell.getByTestId("permission-override-note")).toHaveCount(0);
});

test("raw policy viewer surfaces the live PolicyDocument and writable-permission count", async ({
  page,
  backendWithAdmin,
}) => {
  await loginViaUi(page, backendWithAdmin.baseUrl);
  await page.goto(`${backendWithAdmin.baseUrl}/wrappers?store=default`);

  // The writable-permission count badge lives in the page header now
  // (always visible), not inside the raw-policy dialog.
  await expect(page.getByTestId("writable-permissions-badge")).toHaveText(
    /0 saved writable of 0/i,
  );

  await page.getByRole("button", { name: /view raw policy/i }).click();
  const dlg = page.getByRole("dialog", { name: /raw policy: default/i });
  await expect(dlg).toBeVisible();
  await expect(dlg.getByTestId("policy-json")).toContainText('"permissions"');

  await dlg.getByText("Close", { exact: true }).click();
  await expect(dlg).toBeHidden();

  // Seed a writable permission. The raw policy reads policy.permissions
  // directly and the header badge counts them.
  const clientInstanceId = await createClientInstance(page, backendWithAdmin);
  const resourceId = await seedPair(
    page,
    "default",
    "gh-authorized",
    "github/token",
  );
  await upsertPermission(
    page,
    "default",
    clientInstanceId,
    "gh-authorized",
    resourceId,
    { kind: "unlocked" },
    true,
  );

  await page.reload();
  await expect(page.getByTestId("writable-permissions-badge")).toHaveText(
    /1 saved writable of 1/i,
  );

  await page.getByRole("button", { name: /view raw policy/i }).click();
  const dlg2 = page.getByRole("dialog", { name: /raw policy: default/i });
  await expect(dlg2.getByTestId("policy-json")).toContainText(
    '"writable": true',
  );
});

import type { Page, Request } from "@playwright/test";

import { expect, loginViaUi, test } from "./fixtures";

const STALE_STORE = "stale-store";
const DEFAULT_STORE = "default";

const PAGES = [
  {
    path: "/secrets",
    selectTestId: "secrets-store-select",
    detailPath: (storeId: string) =>
      `/v1/admin/stores/${storeId}/secrets-view`,
    defaultContent: /no resources are known to this store yet/i,
  },
  {
    path: "/wrappers",
    selectTestId: "wrappers-store-select",
    detailPath: (storeId: string) =>
      `/v1/admin/stores/${storeId}/wrapper-permissions-view`,
    defaultContent: /no wrapper\/resource pairs are registered/i,
  },
] as const;

function isExactStoreDetail(request: Request): boolean {
  if (request.method() !== "GET") return false;
  return /^\/v1\/admin\/stores\/[^/]+\/(secrets-view|wrapper-permissions-view)$/.test(
    new URL(request.url()).pathname,
  );
}

function captureStoreDetails(page: Page): {
  paths: string[];
  stop: () => void;
} {
  const paths: string[] = [];
  const listener = (request: Request) => {
    if (isExactStoreDetail(request)) {
      paths.push(new URL(request.url()).pathname);
    }
  };
  page.on("request", listener);
  return {
    paths,
    stop: () => page.off("request", listener),
  };
}

async function settleOnNeutralPage(page: Page, baseUrl: string): Promise<void> {
  await page.goto(`${baseUrl}/profile`, { waitUntil: "networkidle" });
  await expect(page.getByRole("heading", { name: "Profile" })).toBeVisible();
}

function currentPath(page: Page): string {
  const url = new URL(page.url());
  return url.pathname + url.search + url.hash;
}

test("stale store URLs use the real default store without requesting stale detail", async ({
  page,
  backendWithAdmin,
}) => {
  await loginViaUi(page, backendWithAdmin.baseUrl);

  for (const pageCase of PAGES) {
    await settleOnNeutralPage(page, backendWithAdmin.baseUrl);
    const observation = captureStoreDetails(page);
    await page.goto(
      `${backendWithAdmin.baseUrl}${pageCase.path}?store=${STALE_STORE}&keep=yes#section`,
    );

    await expect(page.getByTestId(pageCase.selectTestId)).toHaveValue(DEFAULT_STORE);
    await expect(page.getByText(pageCase.defaultContent)).toBeVisible();
    await expect
      .poll(
        () =>
          observation.paths.filter(
            (path) => path === pageCase.detailPath(DEFAULT_STORE),
          ).length,
      )
      .toBeGreaterThanOrEqual(1);
    expect(
      observation.paths.filter(
        (path) => path === pageCase.detailPath(STALE_STORE),
      ),
    ).toHaveLength(0);
    expect(currentPath(page)).toBe(
      `${pageCase.path}?store=${DEFAULT_STORE}&keep=yes#section`,
    );
    observation.stop();
  }
});

test("a structured store-list failure preserves the URL and starts no store detail", async ({
  page,
  backendWithAdmin,
}) => {
  await loginViaUi(page, backendWithAdmin.baseUrl);
  await page.route("**/v1/admin/stores", async (route) => {
    const requestUrl = new URL(route.request().url());
    if (route.request().method() === "GET" && requestUrl.pathname === "/v1/admin/stores") {
      await route.fulfill({
        status: 503,
        contentType: "application/json",
        body: JSON.stringify({
          code: "store_registry_unavailable",
          message: "Store registry unavailable.",
        }),
      });
      return;
    }
    await route.continue();
  });

  for (const pageCase of PAGES) {
    await settleOnNeutralPage(page, backendWithAdmin.baseUrl);
    const observation = captureStoreDetails(page);
    await page.goto(
      `${backendWithAdmin.baseUrl}${pageCase.path}?store=${STALE_STORE}&keep=yes#section`,
    );

    await expect(page.getByText("Store registry unavailable.")).toBeVisible();
    await expect(page.getByText(/grant|ask an admin for access/i)).toHaveCount(0);
    await expect(page.getByText(pageCase.defaultContent)).toHaveCount(0);
    expect(currentPath(page)).toBe(
      `${pageCase.path}?store=${STALE_STORE}&keep=yes#section`,
    );
    expect(observation.paths).toEqual([]);
    observation.stop();
  }
});

test("an empty store list removes only store and starts no store detail", async ({
  page,
  backendWithAdmin,
}) => {
  await loginViaUi(page, backendWithAdmin.baseUrl);
  await page.route("**/v1/admin/stores", async (route) => {
    const requestUrl = new URL(route.request().url());
    if (route.request().method() === "GET" && requestUrl.pathname === "/v1/admin/stores") {
      await route.fulfill({
        status: 200,
        contentType: "application/json",
        body: JSON.stringify({ items: [] }),
      });
      return;
    }
    await route.continue();
  });

  for (const pageCase of PAGES) {
    await settleOnNeutralPage(page, backendWithAdmin.baseUrl);
    const observation = captureStoreDetails(page);
    await page.goto(
      `${backendWithAdmin.baseUrl}${pageCase.path}?store=${STALE_STORE}&keep=yes#section`,
    );

    await expect(
      page.getByText("No stores are configured. Create one on the Stores page."),
    ).toBeVisible();
    await expect(page).toHaveURL(
      `${backendWithAdmin.baseUrl}${pageCase.path}?keep=yes#section`,
    );
    expect(observation.paths).toEqual([]);
    observation.stop();
  }
});

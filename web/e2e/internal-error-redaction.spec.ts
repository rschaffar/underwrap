import { chmodSync, statSync } from "node:fs";

import type { Response } from "@playwright/test";

import { expect, loginViaUi, test } from "./fixtures";

const PUBLIC_MESSAGE = "could not inspect store contents";
const ERROR_ENVELOPE = {
  code: "list_store_failed",
  message: PUBLIC_MESSAGE,
};

function permissionBits(path: string): number {
  return statSync(path).mode & 0o7777;
}

function isStoreListResponse(response: Response): boolean {
  const request = response.request();
  return (
    request.method() === "GET" &&
    new URL(response.url()).pathname === "/v1/admin/stores"
  );
}

test("real store-list failures redact backend paths and recover after exact mode restoration", async ({
  page,
  backendWithAdmin,
}) => {
  const uid = process.getuid?.();
  expect(
    uid,
    "the chmod witness requires a non-root test process",
  ).toBeDefined();
  expect(uid, "the chmod witness requires a non-root test process").not.toBe(0);

  await loginViaUi(page, backendWithAdmin.baseUrl);
  await page.goto(`${backendWithAdmin.baseUrl}/profile`, {
    waitUntil: "networkidle",
  });
  await expect(page.getByRole("heading", { name: "Profile" })).toBeVisible();

  const originalMode = permissionBits(backendWithAdmin.storeDir);
  const uiResponses: Response[] = [];
  const captureStoreList = (response: Response) => {
    if (isStoreListResponse(response)) uiResponses.push(response);
  };

  try {
    chmodSync(backendWithAdmin.storeDir, 0);
    expect(permissionBits(backendWithAdmin.storeDir)).toBe(0);

    const direct = await page.evaluate(async () => {
      const response = await fetch("/v1/admin/stores", {
        credentials: "include",
      });
      return { status: response.status, body: await response.json() };
    });
    expect(direct).toEqual({ status: 500, body: ERROR_ENVELOPE });
    expect(JSON.stringify(direct.body)).not.toContain(backendWithAdmin.tmpDir);

    page.on("response", captureStoreList);
    await page.goto(`${backendWithAdmin.baseUrl}/stores`);
    const alert = page.getByRole("alert");
    await expect(alert).toHaveText(PUBLIC_MESSAGE);
    await expect(alert).not.toContainText(backendWithAdmin.tmpDir);
    await expect.poll(() => uiResponses.length).toBe(2);

    const uiFailures = await Promise.all(
      uiResponses.map(async (response) => ({
        status: response.status(),
        body: await response.json(),
      })),
    );
    expect(uiFailures).toEqual([
      { status: 500, body: ERROR_ENVELOPE },
      { status: 500, body: ERROR_ENVELOPE },
    ]);
    expect(JSON.stringify(uiFailures)).not.toContain(backendWithAdmin.tmpDir);
  } finally {
    page.off("response", captureStoreList);
    chmodSync(backendWithAdmin.storeDir, originalMode);
    expect(permissionBits(backendWithAdmin.storeDir)).toBe(originalMode);
  }

  const restoredStatus = await page.evaluate(async () => {
    const response = await fetch("/v1/admin/stores", {
      credentials: "include",
    });
    return response.status;
  });
  expect(restoredStatus).toBe(200);
});

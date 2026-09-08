import { afterEach, describe, expect, it, vi } from "vitest";
import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";

import { StoresPage } from "./stores/page";

function mockJson(status: number, body: unknown): Response {
  return new Response(JSON.stringify(body), {
    status,
    headers: { "Content-Type": "application/json" },
  });
}

function withQuery(node: React.ReactElement, qc = new QueryClient({
  defaultOptions: { queries: { retry: false } },
})) {
  return <QueryClientProvider client={qc}>{node}</QueryClientProvider>;
}

function containsString(value: unknown, expected: string): boolean {
  if (typeof value === "string") return value.includes(expected);
  if (Array.isArray(value)) {
    return value.some((entry) => containsString(entry, expected));
  }
  if (value && typeof value === "object") {
    return Object.values(value).some((entry) => containsString(entry, expected));
  }
  return false;
}

afterEach(() => {
  vi.restoreAllMocks();
  localStorage.clear();
  sessionStorage.clear();
  window.history.replaceState({}, "", "/");
});

describe("StoresPage", () => {
  const sampleList = {
    items: [
      {
        id: "store_work",
        name: "work",
        backend: "encrypted" as const,
        description: "Primary store",
        created_at: undefined,
        lifecycle_state: "unlocked" as const,
        usage: { mapped_client_instance_count: 0, has_secrets: false },
        actions: {
          can_update: true,
          can_migrate: true,
          can_rotate_key: true,
          can_unlock: false,
          can_lock: true,
          can_delete_now: true,
          can_force_delete: false,
        },
      },
      {
        id: "store_scratch",
        name: "scratch",
        backend: "plain" as const,
        description: undefined,
        created_at: 100,
        lifecycle_state: "ready" as const,
        usage: { mapped_client_instance_count: 0, has_secrets: true },
        actions: {
          can_update: true,
          can_migrate: true,
          can_rotate_key: false,
          can_unlock: false,
          can_lock: false,
          can_delete_now: false,
          can_force_delete: true,
        },
      },
      {
        id: "store_shared",
        name: "shared",
        backend: "encrypted" as const,
        description: "In use",
        created_at: 200,
        lifecycle_state: "locked" as const,
        usage: { mapped_client_instance_count: 2, has_secrets: true },
        actions: {
          can_update: true,
          can_migrate: false,
          can_rotate_key: false,
          can_unlock: true,
          can_lock: false,
          can_delete_now: false,
          can_force_delete: false,
        },
      },
    ],
  };

  it("enables edit/delete on mutable rows when actions allow it", async () => {
    vi.spyOn(globalThis, "fetch").mockResolvedValue(
      mockJson(200, sampleList),
    );
    render(
      withQuery(
        <StoresPage />,
      ),
    );
    const workRow = (await screen.findByText("work")).closest("tr")!;
    expect(
      within(workRow as HTMLElement).getByRole("button", { name: /^edit$/i }),
    ).toBeEnabled();
    expect(
      within(workRow as HTMLElement).getByRole("button", { name: /^delete$/i }),
    ).toBeEnabled();
  });

  it("admin can open the create dialog", async () => {
    const user = userEvent.setup();
    vi.spyOn(globalThis, "fetch").mockResolvedValue(
      mockJson(200, sampleList),
    );
    render(
      withQuery(
        <StoresPage />,
      ),
    );
    await screen.findByText("work");
    await user.click(screen.getByRole("button", { name: /create store/i }));
    expect(
      await screen.findByRole("dialog", { name: /create store/i }),
    ).toBeVisible();
  });

  it("delete dialog requires force confirmation for non-empty stores", async () => {
    const user = userEvent.setup();
    vi.spyOn(globalThis, "fetch").mockResolvedValue(
      mockJson(200, sampleList),
    );
    render(
      withQuery(
        <StoresPage />,
      ),
    );
    await screen.findByText("scratch");
    const scratchRow = screen.getByText("scratch").closest("tr")!;
    await user.click(
      within(scratchRow as HTMLElement).getByRole("button", {
        name: /^delete$/i,
      }),
    );
    const dlg = await screen.findByRole("dialog", { name: /delete store/i });
    const deleteBtn = within(dlg).getByRole("button", { name: /force delete/i });
    // Force-delete button stays disabled until the operator ticks the
    // "force delete" checkbox; the type-to-confirm gate is gone now.
    expect(deleteBtn).toBeDisabled();
    await user.click(within(dlg).getByLabelText(/force delete even if the store contains secrets/i));
    expect(deleteBtn).toBeEnabled();
  });

  it("shows a Locked badge and Unlock button for locked encrypted stores", async () => {
    vi.spyOn(globalThis, "fetch").mockResolvedValue(
      mockJson(200, sampleList),
    );
    render(
      withQuery(
        <StoresPage />,
      ),
    );
    const sharedRow = (await screen.findByText("shared")).closest("tr")!;
    expect(
      sharedRow.querySelector('[data-testid="store-status-store_shared"]')!
        .textContent,
    ).toMatch(/locked/i);
    expect(
      sharedRow.querySelector('[data-testid="store-unlock-store_shared"]'),
    ).toBeTruthy();
    // Locked rows hide the Lock button (you can only unlock what's locked).
    expect(
      sharedRow.querySelector('[data-testid="store-lock-store_shared"]'),
    ).toBeFalsy();
  });

  it("submits the hex key via postStoreUnlock when the operator unlocks a store", async () => {
    const fetchMock = vi.spyOn(globalThis, "fetch");
    fetchMock.mockImplementation(async (input) => {
      const url = typeof input === "string" ? input : (input as Request).url;
      if (url.endsWith("/v1/admin/stores")) {
        return mockJson(200, sampleList);
      }
      if (url.includes("/unlock")) {
        return mockJson(200, { ok: true });
      }
      throw new Error(`unexpected fetch: ${url}`);
    });
    const user = userEvent.setup();
    render(
      withQuery(
        <StoresPage />,
      ),
    );
    const unlockBtn = await screen.findByTestId("store-unlock-store_shared");
    await user.click(unlockBtn);
    const dlg = await screen.findByTestId("store-unlock-dialog");
    const input = dlg.querySelector(
      '[data-testid="store-unlock-key"]',
    ) as HTMLInputElement;
    const sampleHex =
      "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
    await user.type(input, sampleHex);
    const submit = await screen.findByTestId("store-unlock-submit");
    await user.click(submit);

    // Assert that postStoreUnlock fired against the right path with
    // the right body. openapi-fetch passes a `Request` object as the
    // first arg, so we read URL/method/body off it.
    const unlockReqs = fetchMock.mock.calls
      .map(([first]) => first as Request)
      .filter((req) => req.url.endsWith("/v1/admin/stores/store_shared/unlock"));
    expect(unlockReqs).toHaveLength(1);
    const req = unlockReqs[0]!;
    expect(req.method).toBe("POST");
    const body = JSON.parse(await req.text());
    expect(body).toEqual({ key_hex: sampleHex });
  });

  it("disables delete when a store is still mapped to client instances", async () => {
    vi.spyOn(globalThis, "fetch").mockResolvedValue(
      mockJson(200, sampleList),
    );
    render(
      withQuery(
        <StoresPage />,
      ),
    );
    const sharedRow = (await screen.findByText("shared")).closest("tr")!;
    // Look up Delete by name rather than position — the row may also
    // include an Unlock button when the store is locked.
    const deleteBtn = within(sharedRow as HTMLElement).getByRole("button", {
      name: /^delete$/i,
    }) as HTMLButtonElement;
    expect(deleteBtn).toBeDisabled();
    expect(deleteBtn).toHaveAttribute(
      "title",
      expect.stringMatching(/mapped to 2 client instances/i),
    );
  });

  function reqInfo(input: RequestInfo | URL): { url: string; method: string } {
    if (typeof input === "string") return { url: input, method: "GET" };
    if (input instanceof URL) return { url: input.toString(), method: "GET" };
    const req = input as Request;
    return { url: req.url, method: req.method.toUpperCase() };
  }

  it("generates, exactly confirms, and creates encrypted stores without caching the key", async () => {
    const generatedKey = "a".repeat(64);
    const requests: Request[] = [];
    const user = userEvent.setup();
    const queryClient = new QueryClient({
      defaultOptions: { queries: { retry: false } },
    });
    vi.spyOn(globalThis, "fetch").mockImplementation(async (input) => {
      const request = input as Request;
      requests.push(request);
      const { url, method } = reqInfo(request);
      if (url.endsWith("/v1/admin/stores") && method === "GET") {
        return mockJson(200, sampleList);
      }
      if (url.endsWith("/v1/admin/store-keys") && method === "POST") {
        return mockJson(200, { key_hex: generatedKey });
      }
      if (url.endsWith("/v1/admin/stores") && method === "POST") {
        return mockJson(200, {
          store: { ...sampleList.items[0], id: "store_new", name: "vault" },
        });
      }
      throw new Error(`unexpected fetch: ${method} ${url}`);
    });
    render(withQuery(<StoresPage />, queryClient));
    await screen.findByText("work");
    await user.click(screen.getByRole("button", { name: /create store/i }));
    const createDialog = await screen.findByRole("dialog", {
      name: /create store/i,
    });
    await user.type(within(createDialog).getByLabelText(/^name$/i), "vault");
    await user.selectOptions(
      within(createDialog).getByLabelText(/backend/i),
      "encrypted",
    );
    await user.click(
      within(createDialog).getByRole("button", {
        name: /generate and review key/i,
      }),
    );

    const confirmation = await screen.findByTestId("key-confirmation-dialog");
    expect(within(confirmation).getByTestId("generated-store-key")).toHaveTextContent(
      generatedKey,
    );
    expect(
      requests.filter((request) =>
        request.url.endsWith("/v1/admin/stores"),
      ),
    ).toHaveLength(1);

    const reentry = within(confirmation).getByLabelText(/re-enter the exact/i);
    await user.type(reentry, ` ${generatedKey}`);
    await user.click(
      screen.getByRole("button", { name: /^create encrypted store$/i }),
    );
    expect(
      await within(confirmation).findByText(/does not exactly match/i),
    ).toBeVisible();
    expect(
      requests.filter(
        (request) =>
          request.method === "POST" &&
          request.url.endsWith("/v1/admin/stores"),
      ),
    ).toHaveLength(0);

    await user.clear(reentry);
    await user.type(reentry, generatedKey);
    await user.click(
      screen.getByRole("button", { name: /^create encrypted store$/i }),
    );

    const createRequests = requests.filter(
      (request) =>
        request.method === "POST" && request.url.endsWith("/v1/admin/stores"),
    );
    expect(createRequests).toHaveLength(1);
    expect(JSON.parse(await createRequests[0]!.text())).toEqual({
      name: "vault",
      backend: "encrypted",
      key_hex: generatedKey,
    });
    expect(screen.queryByTestId("generated-store-key")).not.toBeInTheDocument();
    expect(queryClient.getMutationCache().getAll()).toHaveLength(0);
    expect(window.location.href).not.toContain(generatedKey);
    expect(localStorage.length).toBe(0);
    expect(sessionStorage.length).toBe(0);
    expect(queryClient.getQueryData(["admin", "stores"])).toEqual(sampleList);
  });

  it("creates a plain store without generating or sending a key", async () => {
    const requests: Request[] = [];
    const user = userEvent.setup();
    vi.spyOn(globalThis, "fetch").mockImplementation(async (input) => {
      const request = input as Request;
      requests.push(request);
      const { url, method } = reqInfo(request);
      if (url.endsWith("/v1/admin/stores") && method === "GET") {
        return mockJson(200, sampleList);
      }
      if (url.endsWith("/v1/admin/stores") && method === "POST") {
        return mockJson(200, {
          store: { ...sampleList.items[1], id: "store_new", name: "plain" },
        });
      }
      throw new Error(`unexpected fetch: ${method} ${url}`);
    });
    render(withQuery(<StoresPage />));
    await screen.findByText("work");
    await user.click(screen.getByRole("button", { name: /create store/i }));
    const dialog = await screen.findByRole("dialog", { name: /create store/i });
    await user.type(within(dialog).getByLabelText(/^name$/i), "plain");
    await user.click(within(dialog).getByRole("button", { name: /^create$/i }));

    const createRequest = requests.find(
      (request) =>
        request.method === "POST" && request.url.endsWith("/v1/admin/stores"),
    );
    expect(createRequest).toBeDefined();
    expect(JSON.parse(await createRequest!.text())).toEqual({
      name: "plain",
      backend: "plain",
    });
    expect(
      requests.some((request) => request.url.endsWith("/v1/admin/store-keys")),
    ).toBe(false);
  });

  it("clears an encrypted-creation key on pre-request cancellation", async () => {
    const generatedKey = "c".repeat(64);
    const requests: Request[] = [];
    const user = userEvent.setup();
    vi.spyOn(globalThis, "fetch").mockImplementation(async (input) => {
      const request = input as Request;
      requests.push(request);
      const { url, method } = reqInfo(request);
      if (url.endsWith("/v1/admin/stores") && method === "GET") {
        return mockJson(200, sampleList);
      }
      if (url.endsWith("/v1/admin/store-keys") && method === "POST") {
        return mockJson(200, { key_hex: generatedKey });
      }
      throw new Error(`unexpected fetch: ${method} ${url}`);
    });
    render(withQuery(<StoresPage />));
    await screen.findByText("work");
    await user.click(screen.getByRole("button", { name: /create store/i }));
    const dialog = await screen.findByRole("dialog", { name: /create store/i });
    await user.type(within(dialog).getByLabelText(/^name$/i), "cancelled");
    await user.selectOptions(within(dialog).getByLabelText(/backend/i), "encrypted");
    await user.click(
      within(dialog).getByRole("button", { name: /generate and review key/i }),
    );
    expect(await screen.findByTestId("generated-store-key")).toBeVisible();
    await user.click(screen.getByRole("button", { name: /^cancel$/i }));

    expect(screen.queryByTestId("generated-store-key")).not.toBeInTheDocument();
    expect(
      requests.filter(
        (request) =>
          request.method === "POST" && request.url.endsWith("/v1/admin/stores"),
      ),
    ).toHaveLength(0);
  });

  it("retains a key after an ambiguous create failure and retries with the same key", async () => {
    const generatedKey = "d".repeat(64);
    const submittedKeys: string[] = [];
    let createAttempts = 0;
    const user = userEvent.setup();
    vi.spyOn(globalThis, "fetch").mockImplementation(async (input) => {
      const request = input as Request;
      const { url, method } = reqInfo(request);
      if (url.endsWith("/v1/admin/stores") && method === "GET") {
        return mockJson(200, sampleList);
      }
      if (url.endsWith("/v1/admin/store-keys") && method === "POST") {
        return mockJson(200, { key_hex: generatedKey });
      }
      if (url.endsWith("/v1/admin/stores") && method === "POST") {
        createAttempts += 1;
        submittedKeys.push((await request.clone().json()).key_hex);
        if (createAttempts === 1) {
          return mockJson(500, {
            code: "synthetic_ambiguous_failure",
            message: "Synthetic ambiguous failure.",
          });
        }
        return mockJson(200, {
          store: { ...sampleList.items[0], id: "store_new", name: "retry" },
        });
      }
      throw new Error(`unexpected fetch: ${method} ${url}`);
    });
    render(withQuery(<StoresPage />));
    await screen.findByText("work");
    await user.click(screen.getByRole("button", { name: /create store/i }));
    const dialog = await screen.findByRole("dialog", { name: /create store/i });
    await user.type(within(dialog).getByLabelText(/^name$/i), "retry");
    await user.selectOptions(within(dialog).getByLabelText(/backend/i), "encrypted");
    await user.click(
      within(dialog).getByRole("button", { name: /generate and review key/i }),
    );
    const reentry = await screen.findByTestId("store-key-reentry");
    await user.type(reentry, generatedKey);
    await user.click(
      screen.getByRole("button", { name: /^create encrypted store$/i }),
    );

    expect(await screen.findByText("Synthetic ambiguous failure.")).toBeVisible();
    expect(screen.getByTestId("generated-store-key")).toHaveTextContent(generatedKey);
    expect(screen.getByTestId("store-key-reentry")).toHaveValue("");
    expect(screen.getByText(/cannot prove that no data changed/i)).toBeVisible();

    await user.type(screen.getByTestId("store-key-reentry"), generatedKey);
    await user.click(
      screen.getByRole("button", { name: /retry create encrypted store/i }),
    );
    expect(submittedKeys).toEqual([generatedKey, generatedKey]);
    expect(screen.queryByTestId("generated-store-key")).not.toBeInTheDocument();
  });

  it("requires an explicit warning acknowledgement before discarding an ambiguous key", async () => {
    const generatedKey = "e".repeat(64);
    const user = userEvent.setup();
    vi.spyOn(globalThis, "fetch").mockImplementation(async (input) => {
      const { url, method } = reqInfo(input);
      if (url.endsWith("/v1/admin/stores") && method === "GET") {
        return mockJson(200, sampleList);
      }
      if (url.endsWith("/v1/admin/store-keys") && method === "POST") {
        return mockJson(200, { key_hex: generatedKey });
      }
      if (url.endsWith("/v1/admin/stores") && method === "POST") {
        return mockJson(500, {
          code: "synthetic_ambiguous_failure",
          message: "Synthetic ambiguous failure.",
        });
      }
      throw new Error(`unexpected fetch: ${method} ${url}`);
    });
    render(withQuery(<StoresPage />));
    await screen.findByText("work");
    await user.click(screen.getByRole("button", { name: /create store/i }));
    const dialog = await screen.findByRole("dialog", { name: /create store/i });
    await user.type(within(dialog).getByLabelText(/^name$/i), "discard");
    await user.selectOptions(within(dialog).getByLabelText(/backend/i), "encrypted");
    await user.click(
      within(dialog).getByRole("button", { name: /generate and review key/i }),
    );
    await user.type(await screen.findByTestId("store-key-reentry"), generatedKey);
    await user.click(
      screen.getByRole("button", { name: /^create encrypted store$/i }),
    );
    await screen.findByText("Synthetic ambiguous failure.");
    await user.click(screen.getByRole("button", { name: /discard retained key/i }));

    expect(screen.getByText(/may already protect committed or recoverable data/i)).toBeVisible();
    const discard = screen.getByRole("button", { name: /^discard key$/i });
    expect(discard).toBeDisabled();
    await user.click(screen.getByLabelText(/retained key may still be required/i));
    expect(discard).toBeEnabled();
    await user.click(discard);
    expect(screen.queryByTestId("generated-store-key")).not.toBeInTheDocument();
  });

  it("renders lifecycle states and disables backend-denied busy and recovery actions", async () => {
    const unavailableList = {
      items: [
        {
          ...sampleList.items[1],
          id: "store_busy",
          name: "busy-store",
          lifecycle_state: "busy" as const,
          actions: {
            can_update: false,
            can_migrate: false,
            can_rotate_key: false,
            can_unlock: false,
            can_lock: false,
            can_delete_now: false,
            can_force_delete: false,
          },
        },
        {
          ...sampleList.items[1],
          id: "store_recovery",
          name: "recovery-store",
          lifecycle_state: "recovery-required" as const,
          actions: {
            can_update: false,
            can_migrate: false,
            can_rotate_key: false,
            can_unlock: false,
            can_lock: false,
            can_delete_now: false,
            can_force_delete: false,
          },
        },
      ],
    };
    vi.spyOn(globalThis, "fetch").mockResolvedValue(mockJson(200, unavailableList));
    render(withQuery(<StoresPage />));

    for (const name of ["busy-store", "recovery-store"]) {
      const row = (await screen.findByText(name)).closest("tr") as HTMLElement;
      expect(within(row).getByRole("button", { name: /^edit$/i })).toBeDisabled();
      expect(within(row).getByRole("button", { name: /^migrate$/i })).toBeDisabled();
      expect(within(row).getByRole("button", { name: /^delete$/i })).toBeDisabled();
      expect(row).toHaveTextContent(/contents unknown/i);
    }
    expect(screen.getByTestId("store-status-store_busy")).toHaveTextContent("busy");
    expect(screen.getByTestId("store-status-store_recovery")).toHaveTextContent(
      "recovery-required",
    );
  });

  it("shows unlock-first migration guidance for a locked store without sending migration", async () => {
    const requests: Request[] = [];
    const user = userEvent.setup();
    vi.spyOn(globalThis, "fetch").mockImplementation(async (input) => {
      requests.push(input as Request);
      return mockJson(200, sampleList);
    });
    render(withQuery(<StoresPage />));
    const row = (await screen.findByText("shared")).closest("tr") as HTMLElement;
    await user.click(within(row).getByRole("button", { name: /^migrate$/i }));
    const dialog = await screen.findByRole("dialog", { name: /migrate store: shared/i });
    expect(dialog).toHaveTextContent(/unlock this store first/i);
    expect(
      requests.some((request) => request.url.includes("/migrate")),
    ).toBe(false);
  });

  it("migrates plain to encrypted only after generation and exact re-entry", async () => {
    const generatedKey = "f".repeat(64);
    const requests: Request[] = [];
    const user = userEvent.setup();
    window.history.replaceState({}, "", "/stores?store=store_scratch&keep=yes");
    vi.spyOn(globalThis, "fetch").mockImplementation(async (input) => {
      const request = input as Request;
      requests.push(request);
      const { url, method } = reqInfo(request);
      if (url.endsWith("/v1/admin/stores") && method === "GET") {
        return mockJson(200, sampleList);
      }
      if (url.endsWith("/v1/admin/store-keys") && method === "POST") {
        return mockJson(200, { key_hex: generatedKey });
      }
      if (url.endsWith("/v1/admin/stores/store_scratch/migrate") && method === "POST") {
        return mockJson(200, {
          store: {
            ...sampleList.items[1],
            backend: "encrypted",
            lifecycle_state: "unlocked",
          },
        });
      }
      throw new Error(`unexpected fetch: ${method} ${url}`);
    });
    render(withQuery(<StoresPage />));
    const row = (await screen.findByText("scratch")).closest("tr") as HTMLElement;
    await user.click(
      within(row).getByRole("button", { name: /migrate to encrypted/i }),
    );
    const dialog = await screen.findByRole("dialog", {
      name: /migrate to encrypted: scratch/i,
    });
    expect(dialog).toHaveTextContent(/quiesce wrapper traffic/i);
    expect(dialog).toHaveTextContent(/plaintext source data under a private backup/i);
    await user.click(
      within(dialog).getByRole("button", { name: /generate and review key/i }),
    );
    expect(
      requests.some((request) => request.url.includes("/migrate")),
    ).toBe(false);
    await user.type(await screen.findByTestId("store-key-reentry"), generatedKey);
    await user.click(
      screen.getByRole("button", { name: /^migrate to encrypted$/i }),
    );

    const migration = requests.find((request) => request.url.includes("/migrate"));
    expect(migration).toBeDefined();
    expect(JSON.parse(await migration!.text())).toEqual({
      backend: "encrypted",
      key_hex: generatedKey,
    });
    expect(window.location.pathname + window.location.search).toBe(
      "/stores?store=store_scratch&keep=yes",
    );
    expect(screen.queryByTestId("generated-store-key")).not.toBeInTheDocument();
  });

  it("retains and reuses the same key when reconciliation keeps the source unchanged", async () => {
    const generatedKey = "1".repeat(64);
    const submittedKeys: string[] = [];
    let attempts = 0;
    const user = userEvent.setup();
    vi.spyOn(globalThis, "fetch").mockImplementation(async (input) => {
      const request = input as Request;
      const { url, method } = reqInfo(request);
      if (url.endsWith("/v1/admin/stores") && method === "GET") {
        return mockJson(200, sampleList);
      }
      if (url.endsWith("/v1/admin/store-keys") && method === "POST") {
        return mockJson(200, { key_hex: generatedKey });
      }
      if (url.endsWith("/v1/admin/stores/store_scratch/migrate") && method === "POST") {
        attempts += 1;
        submittedKeys.push((await request.clone().json()).key_hex);
        if (attempts === 1) {
          return mockJson(500, {
            code: "synthetic_ambiguous_failure",
            message: "Synthetic ambiguous failure.",
          });
        }
        return mockJson(200, {
          store: {
            ...sampleList.items[1],
            backend: "encrypted",
            lifecycle_state: "unlocked",
          },
        });
      }
      throw new Error(`unexpected fetch: ${method} ${url}`);
    });
    render(withQuery(<StoresPage />));
    const row = (await screen.findByText("scratch")).closest("tr") as HTMLElement;
    await user.click(
      within(row).getByRole("button", { name: /migrate to encrypted/i }),
    );
    const dialog = await screen.findByRole("dialog", {
      name: /migrate to encrypted: scratch/i,
    });
    await user.click(
      within(dialog).getByRole("button", { name: /generate and review key/i }),
    );
    fireEvent.change(await screen.findByTestId("store-key-reentry"), {
      target: { value: generatedKey },
    });
    await user.click(
      screen.getByRole("button", { name: /^migrate to encrypted$/i }),
    );
    expect(await screen.findByText(/synthetic ambiguous failure/i)).toBeVisible();
    expect(screen.getByTestId("generated-store-key")).toHaveTextContent(generatedKey);
    expect(screen.getByTestId("store-key-reentry")).toHaveValue("");

    fireEvent.change(screen.getByTestId("store-key-reentry"), {
      target: { value: generatedKey },
    });
    await user.click(
      screen.getByRole("button", { name: /retry migrate to encrypted/i }),
    );
    expect(submittedKeys).toEqual([generatedKey, generatedKey]);
    expect(screen.queryByTestId("generated-store-key")).not.toBeInTheDocument();
  });

  it("retains the key but disables retry after recovery-required reconciliation", async () => {
    const generatedKey = "2".repeat(64);
    let listRequests = 0;
    let migrationAttempts = 0;
    const user = userEvent.setup();
    const recoveryStore = {
      ...sampleList.items[1],
      backend: "encrypted" as const,
      lifecycle_state: "recovery-required" as const,
      actions: {
        can_update: false,
        can_migrate: false,
        can_rotate_key: false,
        can_unlock: false,
        can_lock: false,
        can_delete_now: false,
        can_force_delete: false,
      },
    };
    vi.spyOn(globalThis, "fetch").mockImplementation(async (input) => {
      const request = input as Request;
      const { url, method } = reqInfo(request);
      if (url.endsWith("/v1/admin/stores") && method === "GET") {
        listRequests += 1;
        return mockJson(200, {
          items:
            listRequests === 1
              ? sampleList.items
              : sampleList.items.map((item) =>
                  item.id === recoveryStore.id ? recoveryStore : item,
                ),
        });
      }
      if (url.endsWith("/v1/admin/store-keys") && method === "POST") {
        return mockJson(200, { key_hex: generatedKey });
      }
      if (url.endsWith("/v1/admin/stores/store_scratch/migrate") && method === "POST") {
        migrationAttempts += 1;
        return mockJson(500, {
          code: "migration_recovery_required",
          message: "Trusted-host recovery is required.",
        });
      }
      throw new Error(`unexpected fetch: ${method} ${url}`);
    });

    render(withQuery(<StoresPage />));
    const row = (await screen.findByText("scratch")).closest("tr") as HTMLElement;
    await user.click(
      within(row).getByRole("button", { name: /migrate to encrypted/i }),
    );
    await user.click(
      within(await screen.findByRole("dialog", { name: /migrate to encrypted/i }))
        .getByRole("button", { name: /generate and review key/i }),
    );
    await user.type(await screen.findByTestId("store-key-reentry"), generatedKey);
    await user.click(
      screen.getByRole("button", { name: /^migrate to encrypted$/i }),
    );

    const retry = screen.getByRole("button", {
      name: /retry migrate to encrypted/i,
    });
    await waitFor(() => expect(retry).toBeDisabled());
    expect(screen.getByTestId("generated-store-key")).toBeVisible();
    expect(screen.getByText(/retain the key because it may protect recoverable data/i)).toBeVisible();
    expect(
      screen.getByRole("button", { name: /discard retained key/i }),
    ).toBeEnabled();
    expect(migrationAttempts).toBe(1);
  });

  it("retains the key but disables retry after already-committed reconciliation", async () => {
    const generatedKey = "3".repeat(64);
    let listRequests = 0;
    let migrationAttempts = 0;
    const user = userEvent.setup();
    const migratedStore = {
      ...sampleList.items[1],
      backend: "encrypted" as const,
      lifecycle_state: "unlocked" as const,
      actions: {
        ...sampleList.items[1].actions,
        can_migrate: true,
        can_rotate_key: true,
        can_lock: true,
      },
    };
    vi.spyOn(globalThis, "fetch").mockImplementation(async (input) => {
      const request = input as Request;
      const { url, method } = reqInfo(request);
      if (url.endsWith("/v1/admin/stores") && method === "GET") {
        listRequests += 1;
        return mockJson(200, {
          items:
            listRequests === 1
              ? sampleList.items
              : sampleList.items.map((item) =>
                  item.id === migratedStore.id ? migratedStore : item,
                ),
        });
      }
      if (url.endsWith("/v1/admin/store-keys") && method === "POST") {
        return mockJson(200, { key_hex: generatedKey });
      }
      if (url.endsWith("/v1/admin/stores/store_scratch/migrate") && method === "POST") {
        migrationAttempts += 1;
        return mockJson(500, {
          code: "synthetic_lost_response",
          message: "Synthetic lost response.",
        });
      }
      throw new Error(`unexpected fetch: ${method} ${url}`);
    });

    render(withQuery(<StoresPage />));
    const row = (await screen.findByText("scratch")).closest("tr") as HTMLElement;
    await user.click(
      within(row).getByRole("button", { name: /migrate to encrypted/i }),
    );
    await user.click(
      within(await screen.findByRole("dialog", { name: /migrate to encrypted/i }))
        .getByRole("button", { name: /generate and review key/i }),
    );
    await user.type(await screen.findByTestId("store-key-reentry"), generatedKey);
    await user.click(
      screen.getByRole("button", { name: /^migrate to encrypted$/i }),
    );

    const retry = screen.getByRole("button", {
      name: /retry migrate to encrypted/i,
    });
    await waitFor(() => expect(retry).toBeDisabled());
    expect(screen.getByTestId("generated-store-key")).toBeVisible();
    expect(screen.getByText(/already uses the target backend/i)).toBeVisible();
    expect(
      screen.getByRole("button", { name: /discard retained key/i }),
    ).toBeEnabled();
    expect(migrationAttempts).toBe(1);
  });

  it("re-enables retained-key controls while reconciliation remains stalled", async () => {
    const generatedKey = "4".repeat(64);
    let listRequests = 0;
    const user = userEvent.setup();
    vi.spyOn(globalThis, "fetch").mockImplementation(async (input) => {
      const request = input as Request;
      const { url, method } = reqInfo(request);
      if (url.endsWith("/v1/admin/stores") && method === "GET") {
        listRequests += 1;
        if (listRequests === 1) return mockJson(200, sampleList);
        return new Promise<Response>(() => {});
      }
      if (url.endsWith("/v1/admin/store-keys") && method === "POST") {
        return mockJson(200, { key_hex: generatedKey });
      }
      if (url.endsWith("/v1/admin/stores/store_scratch/migrate") && method === "POST") {
        return mockJson(500, {
          code: "synthetic_lost_response",
          message: "Synthetic lost response.",
        });
      }
      throw new Error(`unexpected fetch: ${method} ${url}`);
    });

    render(withQuery(<StoresPage />));
    const row = (await screen.findByText("scratch")).closest("tr") as HTMLElement;
    await user.click(
      within(row).getByRole("button", { name: /migrate to encrypted/i }),
    );
    await user.click(
      within(await screen.findByRole("dialog", { name: /migrate to encrypted/i }))
        .getByRole("button", { name: /generate and review key/i }),
    );
    await user.type(await screen.findByTestId("store-key-reentry"), generatedKey);
    await user.click(
      screen.getByRole("button", { name: /^migrate to encrypted$/i }),
    );

    expect(await screen.findByText("Synthetic lost response.")).toBeVisible();
    expect(
      screen.getByRole("button", { name: /retry migrate to encrypted/i }),
    ).toBeEnabled();
    const discard = screen.getByRole("button", {
      name: /discard retained key/i,
    });
    expect(discard).toBeEnabled();
    await user.click(discard);
    expect(
      screen.getByText(/may already protect committed or recoverable data/i),
    ).toBeVisible();
  });

  it("keeps migration confirmation state across unrelated store-list refreshes", async () => {
    const generatedKey = "5".repeat(64);
    const queryClient = new QueryClient({
      defaultOptions: { queries: { retry: false } },
    });
    const user = userEvent.setup();
    vi.spyOn(globalThis, "fetch").mockImplementation(async (input) => {
      const { url, method } = reqInfo(input);
      if (url.endsWith("/v1/admin/stores") && method === "GET") {
        return mockJson(200, sampleList);
      }
      if (url.endsWith("/v1/admin/store-keys") && method === "POST") {
        return mockJson(200, { key_hex: generatedKey });
      }
      throw new Error(`unexpected fetch: ${method} ${url}`);
    });

    render(withQuery(<StoresPage />, queryClient));
    const row = (await screen.findByText("scratch")).closest("tr") as HTMLElement;
    await user.click(
      within(row).getByRole("button", { name: /migrate to encrypted/i }),
    );
    await user.click(
      within(await screen.findByRole("dialog", { name: /migrate to encrypted/i }))
        .getByRole("button", { name: /generate and review key/i }),
    );
    const reentry = await screen.findByTestId("store-key-reentry");
    await user.type(reentry, generatedKey.slice(0, 8));

    act(() => {
      queryClient.setQueryData(["admin", "stores"], {
        items: sampleList.items.map((item) =>
          item.id === "store_work"
            ? { ...item, description: "Refreshed unrelated row" }
            : item,
        ),
      });
    });

    expect(screen.getByTestId("key-confirmation-dialog")).toBeVisible();
    expect(screen.getByTestId("store-key-reentry")).toHaveValue(
      generatedKey.slice(0, 8),
    );
    expect(screen.getByTestId("generated-store-key")).toBeVisible();
  });

  it("disables retained-key retry for missing, busy, and locked source states", async () => {
    const generatedKey = "6".repeat(64);
    const queryClient = new QueryClient({
      defaultOptions: { queries: { retry: false } },
    });
    let listRequests = 0;
    const user = userEvent.setup();
    vi.spyOn(globalThis, "fetch").mockImplementation(async (input) => {
      const request = input as Request;
      const { url, method } = reqInfo(request);
      if (url.endsWith("/v1/admin/stores") && method === "GET") {
        listRequests += 1;
        if (listRequests === 1) return mockJson(200, sampleList);
        return new Promise<Response>(() => {});
      }
      if (url.endsWith("/v1/admin/store-keys") && method === "POST") {
        return mockJson(200, { key_hex: generatedKey });
      }
      if (url.endsWith("/v1/admin/stores/store_scratch/migrate") && method === "POST") {
        return mockJson(500, {
          code: "synthetic_lost_response",
          message: "Synthetic lost response.",
        });
      }
      throw new Error(`unexpected fetch: ${method} ${url}`);
    });

    render(withQuery(<StoresPage />, queryClient));
    const row = (await screen.findByText("scratch")).closest("tr") as HTMLElement;
    await user.click(
      within(row).getByRole("button", { name: /migrate to encrypted/i }),
    );
    await user.click(
      within(await screen.findByRole("dialog", { name: /migrate to encrypted/i }))
        .getByRole("button", { name: /generate and review key/i }),
    );
    await user.type(await screen.findByTestId("store-key-reentry"), generatedKey);
    await user.click(
      screen.getByRole("button", { name: /^migrate to encrypted$/i }),
    );
    await screen.findByText("Synthetic lost response.");

    const retry = screen.getByRole("button", {
      name: /retry migrate to encrypted/i,
    });
    expect(retry).toBeEnabled();

    act(() => {
      queryClient.setQueryData(["admin", "stores"], {
        items: sampleList.items.filter((item) => item.id !== "store_scratch"),
      });
    });
    await waitFor(() => expect(retry).toBeDisabled());
    expect(screen.getByText(/missing from the current Stores list/i)).toBeVisible();
    expect(screen.getByTestId("generated-store-key")).toBeVisible();

    act(() => {
      queryClient.setQueryData(["admin", "stores"], {
        items: sampleList.items.map((item) =>
          item.id === "store_scratch"
            ? {
                ...item,
                lifecycle_state: "busy" as const,
                actions: { ...item.actions, can_migrate: false },
              }
            : item,
        ),
      });
    });
    await waitFor(() => expect(retry).toBeDisabled());
    expect(screen.getByText(/lifecycle operation is busy/i)).toBeVisible();

    act(() => {
      queryClient.setQueryData(["admin", "stores"], {
        items: sampleList.items.map((item) =>
          item.id === "store_scratch"
            ? {
                ...item,
                lifecycle_state: "locked" as const,
                actions: { ...item.actions, can_migrate: false },
              }
            : item,
        ),
      });
    });
    await waitFor(() => expect(retry).toBeDisabled());
    expect(screen.getByText(/source is locked/i)).toBeVisible();

    act(() => {
      queryClient.setQueryData(["admin", "stores"], sampleList);
    });
    await waitFor(() => expect(retry).toBeEnabled());
    expect(screen.getByTestId("generated-store-key")).toBeVisible();
  });

  it("migrates unlocked encrypted to plain without generating or sending a key", async () => {
    const requests: Request[] = [];
    const user = userEvent.setup();
    vi.spyOn(globalThis, "fetch").mockImplementation(async (input) => {
      const request = input as Request;
      requests.push(request);
      const { url, method } = reqInfo(request);
      if (url.endsWith("/v1/admin/stores") && method === "GET") {
        return mockJson(200, sampleList);
      }
      if (url.endsWith("/v1/admin/stores/store_work/migrate") && method === "POST") {
        return mockJson(200, {
          store: {
            ...sampleList.items[0],
            backend: "plain",
            lifecycle_state: "ready",
          },
        });
      }
      throw new Error(`unexpected fetch: ${method} ${url}`);
    });
    render(withQuery(<StoresPage />));
    const row = (await screen.findByText("work")).closest("tr") as HTMLElement;
    await user.click(within(row).getByRole("button", { name: /migrate to plain/i }));
    const dialog = await screen.findByRole("dialog", {
      name: /migrate to plain: work/i,
    });
    expect(dialog).toHaveTextContent(/plaintext target data under a private stage/i);
    await user.click(
      within(dialog).getByRole("button", { name: /^migrate to plain$/i }),
    );

    const migration = requests.find((request) => request.url.includes("/migrate"));
    expect(migration).toBeDefined();
    expect(JSON.parse(await migration!.text())).toEqual({ backend: "plain" });
    expect(
      requests.some((request) => request.url.endsWith("/v1/admin/store-keys")),
    ).toBe(false);
  });

  it("reveals the new key without placing it in mutation-cache data", async () => {
    const rotationKey = "b".repeat(64);
    const queryClient = new QueryClient({
      defaultOptions: { queries: { retry: false } },
    });
    const user = userEvent.setup();
    vi.spyOn(globalThis, "fetch").mockImplementation(async (input) => {
      const { url, method } = reqInfo(input);
      if (url.endsWith("/v1/admin/stores") && method === "GET") {
        return mockJson(200, sampleList);
      }
      if (url.includes("/rotate-key") && method === "POST") {
        return mockJson(200, {
          key_hex: rotationKey,
          warning: "Re-encrypted 3 resources.",
        });
      }
      throw new Error(`unexpected fetch: ${method} ${url}`);
    });
    render(withQuery(<StoresPage />, queryClient));
    const workRow = (await screen.findByText("work")).closest("tr")!;
    await user.click(
      within(workRow as HTMLElement).getByRole("button", {
        name: /rotate key/i,
      }),
    );
    const dialog = await screen.findByRole("dialog", {
      name: /rotate master key/i,
    });
    expect(dialog).toHaveTextContent(/displaced ciphertext generation remains on the backend host/i);
    expect(dialog).toHaveTextContent(/consumes disk/i);
    expect(dialog).toHaveTextContent(/decryptable with its old key/i);
    expect(dialog).toHaveTextContent(/manual cleanup/i);
    await user.click(within(dialog).getByRole("checkbox"));
    await user.click(
      within(dialog).getByRole("button", { name: /^rotate key$/i }),
    );

    const reveal = await screen.findByTestId("key-reveal-dialog");
    expect(within(reveal).getByTestId("key-reveal-value")).toHaveTextContent(
      rotationKey,
    );
    expect(
      queryClient
        .getMutationCache()
        .getAll()
        .some((mutation) => containsString(mutation.state.data, rotationKey)),
    ).toBe(false);
    expect(reveal).toHaveTextContent(/retained old ciphertext consumes backend-host disk/i);
    expect(reveal).toHaveTextContent(/remains decryptable with the old key/i);
    expect(reveal).toHaveTextContent(/closing this dialog discards the one-time local display/i);
    expect(reveal).toHaveTextContent(
      /backend cannot know whether I received, saved, or independently verified/i,
    );
    expect(reveal).not.toHaveTextContent(/I have saved and independently verified/i);
    expect(screen.getByTestId("key-reveal-confirm")).toBeDisabled();
  });

  it("keeps rotation retryable and never reveals a key after rejection", async () => {
    const user = userEvent.setup();
    vi.spyOn(globalThis, "fetch").mockImplementation(async (input) => {
      const { url, method } = reqInfo(input);
      if (url.endsWith("/v1/admin/stores") && method === "GET") {
        return mockJson(200, sampleList);
      }
      if (url.includes("/rotate-key") && method === "POST") {
        return mockJson(503, {
          code: "synthetic_rotation_failure",
          message: "Synthetic rotation failure.",
        });
      }
      throw new Error(`unexpected fetch: ${method} ${url}`);
    });
    render(withQuery(<StoresPage />));
    const workRow = (await screen.findByText("work")).closest("tr")!;
    await user.click(
      within(workRow as HTMLElement).getByRole("button", {
        name: /rotate key/i,
      }),
    );
    const dialog = await screen.findByRole("dialog", {
      name: /rotate master key/i,
    });
    await user.click(within(dialog).getByRole("checkbox"));
    const rotateButton = within(dialog).getByRole("button", {
      name: /^rotate key$/i,
    });

    await user.click(rotateButton);

    expect(await within(dialog).findByRole("alert")).toHaveTextContent(
      "Synthetic rotation failure.",
    );
    expect(dialog).toBeVisible();
    expect(rotateButton).toBeEnabled();
    expect(screen.queryByTestId("key-reveal-dialog")).not.toBeInTheDocument();
  });

  it("shows the cleanup hint after a force-delete leaves data on disk", async () => {
    const user = userEvent.setup();
    vi.spyOn(globalThis, "fetch").mockImplementation(async (input) => {
      const { url, method } = reqInfo(input);
      if (url.endsWith("/v1/admin/stores") && method === "GET") {
        return mockJson(200, sampleList);
      }
      if (url.includes("/v1/admin/stores/store_scratch") && method === "DELETE") {
        return mockJson(200, {
          ok: true,
          cleanup_hint: { data_dir: "/var/lib/uw/scratch", command: "rm -rf /var/lib/uw/scratch" },
        });
      }
      throw new Error(`unexpected fetch: ${method} ${url}`);
    });
    render(withQuery(<StoresPage />));
    const scratchRow = (await screen.findByText("scratch")).closest("tr")!;
    await user.click(
      within(scratchRow as HTMLElement).getByRole("button", { name: /^delete$/i }),
    );
    const dialog = await screen.findByRole("dialog", { name: /delete store/i });
    await user.click(
      within(dialog).getByLabelText(
        /force delete even if the store contains secrets/i,
      ),
    );
    await user.click(within(dialog).getByRole("button", { name: /force delete/i }));

    const hint = await screen.findByRole("dialog", { name: /store deleted/i });
    expect(within(hint).getByText("/var/lib/uw/scratch")).toBeInTheDocument();
    expect(within(hint).getByText("rm -rf /var/lib/uw/scratch")).toBeInTheDocument();
  });

  it("locks an unlocked encrypted store via the lock dialog", async () => {
    const user = userEvent.setup();
    let locked = false;
    vi.spyOn(globalThis, "fetch").mockImplementation(async (input) => {
      const { url, method } = reqInfo(input);
      if (url.endsWith("/v1/admin/stores") && method === "GET") {
        return mockJson(200, sampleList);
      }
      if (url.includes("/v1/admin/stores/store_work/lock") && method === "POST") {
        locked = true;
        return mockJson(200, { ok: true });
      }
      throw new Error(`unexpected fetch: ${method} ${url}`);
    });
    render(withQuery(<StoresPage />));
    const workRow = (await screen.findByText("work")).closest("tr")!;
    await user.click(
      within(workRow as HTMLElement).getByRole("button", { name: /^lock$/i }),
    );
    const dialog = await screen.findByRole("dialog", { name: /lock store: work/i });
    await user.click(within(dialog).getByRole("button", { name: /^lock$/i }));
    expect(locked).toBe(true);
  });
});


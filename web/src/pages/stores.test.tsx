import { afterEach, describe, expect, it, vi } from "vitest";
import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";

import { StoresPage } from "./stores/page";

function mockJson(status: number, body: unknown): Response {
  return new Response(JSON.stringify(body), {
    status,
    headers: { "Content-Type": "application/json" },
  });
}

function withQuery(node: React.ReactElement) {
  const qc = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  return <QueryClientProvider client={qc}>{node}</QueryClientProvider>;
}

afterEach(() => {
  vi.restoreAllMocks();
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
        locked: false,
        usage: { mapped_client_instance_count: 0, has_secrets: false },
        actions: {
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
        locked: false,
        usage: { mapped_client_instance_count: 0, has_secrets: true },
        actions: {
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
        locked: true,
        usage: { mapped_client_instance_count: 2, has_secrets: true },
        actions: {
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

  it("reveals the master key once after creating an encrypted store, gated by the ack checkbox", async () => {
    const user = userEvent.setup();
    vi.spyOn(globalThis, "fetch").mockImplementation(async (input) => {
      const { url, method } = reqInfo(input);
      if (url.endsWith("/v1/admin/stores") && method === "GET") {
        return mockJson(200, sampleList);
      }
      if (url.endsWith("/v1/admin/stores") && method === "POST") {
        return mockJson(200, {
          store: { ...sampleList.items[0], id: "store_new", name: "vault" },
          key_hex: "a".repeat(64),
        });
      }
      throw new Error(`unexpected fetch: ${method} ${url}`);
    });
    render(withQuery(<StoresPage />));
    await screen.findByText("work");
    await user.click(screen.getByRole("button", { name: /create store/i }));
    const dialog = await screen.findByRole("dialog", { name: /create store/i });
    await user.type(within(dialog).getByLabelText(/name/i), "vault");
    await user.selectOptions(
      within(dialog).getByLabelText(/backend/i),
      "encrypted",
    );
    await user.click(within(dialog).getByRole("button", { name: /^create$/i }));

    const reveal = await screen.findByTestId("key-reveal-dialog");
    expect(within(reveal).getByTestId("key-reveal-value")).toHaveTextContent(
      "a".repeat(64),
    );
    // The key must stay until the operator confirms they saved it.
    expect(screen.getByTestId("key-reveal-confirm")).toBeDisabled();
    await user.click(screen.getByTestId("key-reveal-ack"));
    expect(screen.getByTestId("key-reveal-confirm")).toBeEnabled();
  });

  it("reveals the new key after rotating an encrypted store's key", async () => {
    const user = userEvent.setup();
    vi.spyOn(globalThis, "fetch").mockImplementation(async (input) => {
      const { url, method } = reqInfo(input);
      if (url.endsWith("/v1/admin/stores") && method === "GET") {
        return mockJson(200, sampleList);
      }
      if (url.includes("/rotate-key") && method === "POST") {
        return mockJson(200, {
          key_hex: "b".repeat(64),
          warning: "Re-encrypted 3 resources.",
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
      "b".repeat(64),
    );
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


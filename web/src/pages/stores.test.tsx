import { afterEach, describe, expect, it, vi } from "vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";

import { StoresPage } from "./stores";

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
          can_edit_metadata: true,
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
          can_edit_metadata: true,
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
          can_edit_metadata: true,
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
    const buttons = workRow.querySelectorAll("button");
    expect(buttons[0]!).toBeEnabled();
    expect(buttons[1]!).toBeEnabled();
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
    // The encrypted-only key_file field should NOT be visible while
    // backend == plain (the default).
    expect(screen.queryByLabelText(/key file/i)).toBeNull();
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
    const buttons = scratchRow.querySelectorAll("button");
    await user.click(buttons[1]!);
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
    // Rotate is disabled for locked stores — opening the edit dialog
    // and ticking the box is not enough to enable the rotate button.
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
    // the right body.
    const callsToUnlock = fetchMock.mock.calls.filter(([url]) =>
      String(url).endsWith("/v1/admin/stores/store_shared/unlock"),
    );
    expect(callsToUnlock).toHaveLength(1);
    const init = callsToUnlock[0]?.[1] as RequestInit | undefined;
    expect(init?.method).toBe("POST");
    const body = JSON.parse(String(init?.body));
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
});

// Local within helper to avoid pulling testing-library's `within` separately.
function within(scope: HTMLElement) {
  return {
    getByRole: (role: string, options?: { name?: RegExp }) => {
      const all = Array.from(
        scope.querySelectorAll<HTMLElement>("[role], button, a"),
      );
      for (const el of all) {
        const r = el.getAttribute("role") ?? el.tagName.toLowerCase();
        if (r !== role && !(role === "button" && r === "button")) continue;
        if (
          options?.name &&
          !options.name.test(el.textContent ?? "") &&
          !options.name.test(el.getAttribute("aria-label") ?? "")
        ) {
          continue;
        }
        return el;
      }
      throw new Error(`role ${role} not found`);
    },
    getByLabelText: (re: RegExp) => {
      const labels = Array.from(scope.querySelectorAll("label"));
      for (const lbl of labels) {
        if (!re.test(lbl.textContent ?? "")) continue;
        const id = lbl.getAttribute("for");
        if (id) {
          const el = scope.querySelector<HTMLElement>(`#${CSS.escape(id)}`);
          if (el) return el;
        }
        const nested = lbl.querySelector<HTMLElement>("input, textarea, select");
        if (nested) return nested;
      }
      throw new Error(`label ${re} not found`);
    },
  };
}

import { afterEach, describe, expect, it, vi } from "vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";

import {
  StoresPage,
  canMutateStores,
  isStoreReadOnly,
} from "./stores";

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

describe("permission helpers", () => {
  it("admins may mutate; everyone else may not", () => {
    expect(
      canMutateStores({ authenticated: true, role: "admin" }),
    ).toBe(true);
    expect(
      canMutateStores({ authenticated: true, role: "operator" }),
    ).toBe(false);
    expect(
      canMutateStores({ authenticated: false }),
    ).toBe(false);
    expect(canMutateStores(null)).toBe(false);
  });

  it("stores are never read-only by source", () => {
    expect(isStoreReadOnly("dynamic")).toBe(false);
  });
});

describe("StoresPage", () => {
  const sampleList = {
    items: [
      {
        id: "store_work",
        name: "work",
        source: "dynamic" as const,
        backend: "encrypted" as const,
        description: "Primary store",
        created_at: undefined,
        usage: { mapped_client_instance_count: 0, has_secrets: false },
        actions: {
          can_edit_metadata: true,
          can_rotate_key: true,
          can_delete_now: true,
          can_force_delete: false,
        },
      },
      {
        id: "store_scratch",
        name: "scratch",
        source: "dynamic" as const,
        backend: "plain" as const,
        description: undefined,
        created_at: 100,
        usage: { mapped_client_instance_count: 0, has_secrets: true },
        actions: {
          can_edit_metadata: true,
          can_rotate_key: false,
          can_delete_now: false,
          can_force_delete: true,
        },
      },
      {
        id: "store_shared",
        name: "shared",
        source: "dynamic" as const,
        backend: "encrypted" as const,
        description: "In use",
        created_at: 200,
        usage: { mapped_client_instance_count: 2, has_secrets: true },
        actions: {
          can_edit_metadata: true,
          can_rotate_key: true,
          can_delete_now: false,
          can_force_delete: false,
        },
      },
    ],
  };

  it("hides the create button for non-admins", async () => {
    vi.spyOn(globalThis, "fetch").mockResolvedValue(
      mockJson(200, sampleList),
    );
    render(
      withQuery(
        <StoresPage
          principal={{ authenticated: true, role: "operator" }}
        />,
      ),
    );
    // Wait for the table to render.
    await screen.findByText(/^work$/);
    expect(
      screen.queryByRole("button", { name: /create store/i }),
    ).toBeNull();
  });

  it("enables edit/delete on mutable rows when actions allow it", async () => {
    vi.spyOn(globalThis, "fetch").mockResolvedValue(
      mockJson(200, sampleList),
    );
    render(
      withQuery(
        <StoresPage
          principal={{ authenticated: true, role: "admin" }}
        />,
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
        <StoresPage
          principal={{ authenticated: true, role: "admin" }}
        />,
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
        <StoresPage
          principal={{ authenticated: true, role: "admin" }}
        />,
      ),
    );
    await screen.findByText("scratch");
    const scratchRow = screen.getByText("scratch").closest("tr")!;
    const buttons = scratchRow.querySelectorAll("button");
    await user.click(buttons[1]!);
    const dlg = await screen.findByRole("dialog", { name: /delete store/i });
    const deleteBtn = within(dlg).getByRole("button", { name: /force delete/i });
    expect(deleteBtn).toBeDisabled();
    await user.type(within(dlg).getByLabelText(/type/i), "scratch");
    expect(deleteBtn).toBeDisabled();
    await user.click(within(dlg).getByLabelText(/force delete even if the store contains secrets/i));
    expect(deleteBtn).toBeEnabled();
  });

  it("disables delete when a store is still mapped to client instances", async () => {
    vi.spyOn(globalThis, "fetch").mockResolvedValue(
      mockJson(200, sampleList),
    );
    render(
      withQuery(
        <StoresPage
          principal={{ authenticated: true, role: "admin" }}
        />,
      ),
    );
    const sharedRow = (await screen.findByText("shared")).closest("tr")!;
    const buttons = sharedRow.querySelectorAll("button");
    const deleteBtn = buttons[1]!;
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

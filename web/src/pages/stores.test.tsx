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

  it("declarative stores are read-only", () => {
    expect(isStoreReadOnly("declarative")).toBe(true);
    expect(isStoreReadOnly("dynamic")).toBe(false);
  });
});

describe("StoresPage", () => {
  const sampleList = {
    items: [
      {
        name: "work",
        source: "declarative" as const,
        backend: "encrypted" as const,
        description: "Nix-owned",
        created_at: undefined,
      },
      {
        name: "scratch",
        source: "dynamic" as const,
        backend: "plain" as const,
        description: undefined,
        created_at: 100,
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
    await screen.findByText(/work/);
    expect(
      screen.queryByRole("button", { name: /create store/i }),
    ).toBeNull();
  });

  it("disables edit/delete on declarative rows and tooltips why", async () => {
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
    const editBtn = workRow.querySelector("button")!;
    expect(editBtn).toBeDisabled();
    expect(editBtn).toHaveAttribute(
      "title",
      expect.stringMatching(/declarative store/i),
    );
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

  it("delete dialog refuses to enable destructive button until the name is typed", async () => {
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
    // [Edit, Delete] — click Delete.
    await user.click(buttons[1]!);
    const dlg = await screen.findByRole("dialog", { name: /delete store/i });
    const deleteBtn = within(dlg).getByRole("button", { name: /^delete$/i });
    expect(deleteBtn).toBeDisabled();
    await user.type(within(dlg).getByLabelText(/type/i), "scratch");
    expect(deleteBtn).toBeEnabled();
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
        if (re.test(lbl.textContent ?? "")) {
          const id = lbl.getAttribute("for");
          if (id) {
            const el = scope.querySelector<HTMLElement>(`#${CSS.escape(id)}`);
            if (el) return el;
          }
        }
      }
      throw new Error(`label ${re} not found`);
    },
  };
}

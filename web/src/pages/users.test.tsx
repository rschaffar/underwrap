import { afterEach, describe, expect, it, vi } from "vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";

import { UsersPage, canManageUsers } from "./users";

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

// u64 fields are typed as bigint in the ts-rs bindings, but on the
// wire they're plain numbers (JSON has no BigInt). Using `unknown`
// here lets the test JSON-encode them; the page reads them via
// `Number(epoch)`, which works on both branches.
const sampleUsers = {
  users: [
    {
      id: "id-alice",
      username: "alice",
      role: "admin",
      granted_stores: [],
      created_at: 1700_000_000,
      password_changed_at: 1700_000_000,
    },
    {
      id: "id-bob",
      username: "bob",
      role: "operator",
      granted_stores: ["work"],
      created_at: 1710_000_000,
      password_changed_at: 1710_000_000,
    },
  ],
};
const sampleStoresList = {
  items: [
    {
      id: "store_work",
      name: "work",
      source: "dynamic" as const,
      backend: "plain" as const,
      description: undefined,
      created_at: undefined,
    },
  ],
};

function mockUsersAndStoresFetch() {
  vi.spyOn(globalThis, "fetch").mockImplementation((input) => {
    const url = typeof input === "string" ? input : (input as Request).url;
    if (url.includes("/v1/admin/users")) {
      return Promise.resolve(mockJson(200, sampleUsers));
    }
    if (url.includes("/v1/admin/stores")) {
      return Promise.resolve(mockJson(200, sampleStoresList));
    }
    return Promise.resolve(mockJson(404, { code: "not_found" }));
  });
}

describe("permission helper", () => {
  it("only admin operators can manage users", () => {
    expect(canManageUsers({ authenticated: true, role: "admin" })).toBe(true);
    expect(canManageUsers({ authenticated: true, role: "operator" })).toBe(
      false,
    );
    expect(canManageUsers({ authenticated: false })).toBe(false);
    expect(canManageUsers(null)).toBe(false);
  });
});

describe("UsersPage", () => {
  it("hides the manage UI for non-admin sessions", () => {
    render(
      withQuery(
        <UsersPage principal={{ authenticated: true, role: "operator" }} />,
      ),
    );
    expect(
      screen.queryByRole("button", { name: /create user/i }),
    ).toBeNull();
    expect(screen.getByRole("alert").textContent).toMatch(/admin/i);
  });

  it("admin sees the table with the (you) marker on their own row", async () => {
    mockUsersAndStoresFetch();
    render(
      withQuery(
        <UsersPage
          principal={{
            authenticated: true,
            role: "admin",
            username: "alice",
          }}
        />,
      ),
    );
    await screen.findByText("alice");
    const aliceRow = screen.getByText("alice").closest("tr")!;
    expect(aliceRow.textContent).toMatch(/you/i);
    // bob is not me.
    const bobRow = screen.getByText("bob").closest("tr")!;
    expect(bobRow.textContent ?? "").not.toMatch(/you/i);
  });

  it("delete dialog disables the destructive button when targeting the last admin", async () => {
    const user = userEvent.setup();
    mockUsersAndStoresFetch();
    render(
      withQuery(
        <UsersPage
          principal={{
            authenticated: true,
            role: "admin",
            username: "alice",
          }}
        />,
      ),
    );
    await screen.findByText("alice");
    const row = screen.getByText("alice").closest("tr")!;
    const buttons = row.querySelectorAll("button");
    // [Edit, Reset password, Delete]
    await user.click(buttons[2]!);
    const dlg = await screen.findByRole("dialog", {
      name: /delete user: alice/i,
    });
    expect(dlg.textContent).toMatch(/last remaining admin/i);
    const deleteBtns = Array.from(
      dlg.querySelectorAll<HTMLButtonElement>("button"),
    );
    const destructive = deleteBtns.find((b) => /^delete$/i.test(b.textContent ?? ""));
    expect(destructive).toBeTruthy();
    expect(destructive!).toBeDisabled();
  });

  it("create dialog rejects mismatched passwords", async () => {
    const user = userEvent.setup();
    mockUsersAndStoresFetch();
    render(
      withQuery(
        <UsersPage
          principal={{
            authenticated: true,
            role: "admin",
            username: "alice",
          }}
        />,
      ),
    );
    await screen.findByText("alice");
    await user.click(screen.getByRole("button", { name: /create user/i }));
    const dlg = await screen.findByRole("dialog", { name: /create user/i });
    // Form fields by label
    function getLabel(re: RegExp): HTMLElement {
      const labels = Array.from(dlg.querySelectorAll("label"));
      for (const lbl of labels) {
        if (re.test(lbl.textContent ?? "")) {
          const id = lbl.getAttribute("for");
          if (id) {
            const el = dlg.querySelector<HTMLElement>(`#${CSS.escape(id)}`);
            if (el) return el;
          }
        }
      }
      throw new Error(`label ${re} not found`);
    }
    await user.type(getLabel(/^username$/i), "carol");
    await user.type(getLabel(/^password$/i), "hunter22");
    await user.type(getLabel(/confirm password/i), "different-1");
    // Try to submit.
    const submit = Array.from(dlg.querySelectorAll("button")).find(
      (b) => /^create$/i.test(b.textContent ?? ""),
    )!;
    await user.click(submit);
    // Mismatch surface as a form message.
    expect(dlg.textContent).toMatch(/passwords do not match/i);
  });
});

import { afterEach, describe, expect, it, vi } from "vitest";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";

import {
  EnrollmentPage,
  buildEnrollmentCommand,
  parseEnrollmentHints,
} from "./enroll";

function mockJson(status: number, body: unknown): Response {
  return new Response(JSON.stringify(body), {
    status,
    headers: { "Content-Type": "application/json" },
  });
}

function withQuery(node: React.ReactElement) {
  const qc = new QueryClient({
    defaultOptions: { queries: { retry: false }, mutations: { retry: false } },
  });
  return <QueryClientProvider client={qc}>{node}</QueryClientProvider>;
}

afterEach(() => {
  vi.restoreAllMocks();
  window.history.pushState({}, "", "/");
});

describe("enrollment helpers", () => {
  it("parses wrapper deep-link hints", () => {
    expect(parseEnrollmentHints("?host=deepthought&instance=work")).toEqual({
      host: "deepthought",
      instance: "work",
    });
  });

  it("builds the local enrollment command without argv secret exposure", () => {
    const command = buildEnrollmentCommand("/v1/enroll", "enr_abc123", "work");
    expect(command).toContain("sudo underwrap-enroll");
    expect(command).toContain("--invite-id enr_abc123");
    expect(command).toContain("--instance-config /etc/underwrap/instances/work.yaml");
    expect(command).not.toContain("--url");
    expect(command).not.toContain("--ca-file");
    expect(command).not.toContain("--client-cert-out");
    expect(command).not.toContain("--client-key-out");
    expect(command).not.toContain("enrollment_secret");
  });
});

describe("EnrollmentPage", () => {
  it("opens from the wrapper deep link and pre-fills host/instance hints", async () => {
    window.history.pushState({}, "", "/enroll?host=deepthought&instance=work");
    vi.spyOn(globalThis, "fetch").mockResolvedValue(
      mockJson(200, {
        items: [
          {
            id: "store_work",
            name: "work",
            source: "dynamic",
            backend: "plain",
            description: null,
            created_at: 100,
          },
        ],
      }),
    );

    render(
      withQuery(
        <EnrollmentPage principal={{ authenticated: true, role: "admin" }} />,
      ),
    );

    expect(await screen.findByRole("heading", { name: /enroll instance/i })).toBeVisible();
    expect(screen.getByLabelText(/host hint/i)).toHaveValue("deepthought");
    expect(screen.getByLabelText(/instance name/i)).toHaveValue("work");
  });

  it("admins can create a new store inline and auto-select it", async () => {
    window.history.pushState({}, "", "/enroll?host=deepthought&instance=work");
    const user = userEvent.setup();
    let storeListCalls = 0;
    vi.spyOn(globalThis, "fetch").mockImplementation(async (input, init) => {
      const url = String(input);
      const method = init?.method ?? "GET";
      if (url === "/v1/admin/stores" && method === "GET") {
        storeListCalls += 1;
        return mockJson(
          200,
          storeListCalls === 1
            ? { items: [] }
            : {
                items: [
                  {
                    id: "store_work",
                    name: "work",
                    source: "dynamic",
                    backend: "plain",
                    description: null,
                    created_at: 100,
                  },
                ],
              },
        );
      }
      if (url === "/v1/admin/stores" && method === "POST") {
        return mockJson(200, {
          store: {
            id: "store_work",
            name: "work",
            source: "dynamic",
            backend: "plain",
            dir: "/var/lib/underwrap-backend/data/work",
            description: null,
            created_at: 100,
            source_path: "/var/lib/underwrap-backend/stores.json",
          },
        });
      }
      throw new Error(`unexpected fetch ${method} ${url}`);
    });

    render(
      withQuery(
        <EnrollmentPage principal={{ authenticated: true, role: "admin" }} />,
      ),
    );

    await screen.findByRole("button", { name: /create new store inline/i });
    await user.click(screen.getByRole("button", { name: /create new store inline/i }));
    await user.clear(screen.getByLabelText(/store name/i));
    await user.type(screen.getByLabelText(/store name/i), "work");
    await user.clear(screen.getByLabelText(/data directory/i));
    await user.type(
      screen.getByLabelText(/data directory/i),
      "/var/lib/underwrap-backend/data/work",
    );
    await user.click(screen.getByRole("button", { name: /^create store$/i }));

    await waitFor(() => {
      expect(screen.getByLabelText(/^store$/i)).toHaveValue("store_work");
    });
    expect(screen.getByText(/selected store: work/i)).toBeVisible();
  });

  it("renders the exact local command after invite creation", async () => {
    window.history.pushState({}, "", "/enroll?host=deepthought&instance=work");
    const user = userEvent.setup();
    vi.spyOn(globalThis, "fetch").mockImplementation(async (input, init) => {
      const url = String(input);
      const method = init?.method ?? "GET";
      if (url === "/v1/admin/stores" && method === "GET") {
        return mockJson(200, {
          items: [
            {
              id: "store_work",
              name: "work",
              source: "dynamic",
              backend: "plain",
              description: null,
              created_at: 100,
            },
          ],
        });
      }
      if (url === "/v1/admin/enrollment-invites" && method === "POST") {
        return mockJson(200, {
          invite: {
            id: "enr_abc123",
            display_name: "deepthought work",
            hostname_hint: "deepthought",
            instance_name: "work",
            current_store_id: "store_work",
            created_at: 100,
            expires_at: 200,
            status: "pending",
          },
          enrollment_secret: "secret-once",
          enrollment_url: "/v1/enroll",
          enrollment_command: "ignored-by-ui",
        });
      }
      throw new Error(`unexpected fetch ${method} ${url}`);
    });

    render(
      withQuery(
        <EnrollmentPage principal={{ authenticated: true, role: "admin" }} />,
      ),
    );

    await screen.findByRole("option", { name: "work" });
    await waitFor(() => {
      expect(screen.getByLabelText(/^store$/i)).toHaveValue("store_work");
    });
    await user.click(screen.getByRole("button", { name: /create enrollment invite/i }));

    expect(await screen.findByText("secret-once")).toBeVisible();
    expect(screen.getByText(/sudo underwrap-enroll/)).toBeVisible();
    expect(screen.getByText(/--invite-id enr_abc123/)).toBeVisible();
    expect(
      screen.getByText(/--instance-config \/etc\/underwrap\/instances\/work\.yaml/),
    ).toBeVisible();
  });
});

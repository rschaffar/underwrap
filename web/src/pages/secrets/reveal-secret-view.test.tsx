import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";

import { RevealSecretView } from "./reveal-secret-view";

type Response = { material_b64: string | null };

const queryKey = ["test", "revealed-secret"] as const;
const queryClients: QueryClient[] = [];
let originalClipboard: PropertyDescriptor | undefined;
let writeText: ReturnType<typeof vi.fn>;

function renderReveal(materialB64: string | null) {
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  queryClients.push(queryClient);
  const view = render(
    <QueryClientProvider client={queryClient}>
      <RevealSecretView<Response>
        title="Reveal secret"
        description="Test secret"
        queryKey={queryKey}
        queryFn={vi.fn().mockResolvedValue({ material_b64: materialB64 })}
        pickB64={(response) => response.material_b64}
        textareaId="revealed-secret"
        textareaTestId="revealed-secret"
        onClose={() => {}}
      />
    </QueryClientProvider>,
  );
  return { ...view, queryClient };
}

function showSecret() {
  fireEvent.click(screen.getByRole("button", { name: "Show" }));
}

describe("RevealSecretView", () => {
  beforeEach(() => {
    originalClipboard = Object.getOwnPropertyDescriptor(navigator, "clipboard");
    writeText = vi.fn().mockResolvedValue(undefined);
    Object.defineProperty(navigator, "clipboard", {
      configurable: true,
      value: { writeText },
    });
  });

  afterEach(() => {
    for (const queryClient of queryClients.splice(0)) queryClient.clear();
    if (originalClipboard) {
      Object.defineProperty(navigator, "clipboard", originalClipboard);
    } else {
      Reflect.deleteProperty(navigator, "clipboard");
    }
  });

  it("reveals and copies exact Unicode, then releases cached query data", async () => {
    const { queryClient, unmount } = renderReveal("4pyTIMOgIGxhIG1vZGU=");
    await showSecret();

    const textarea = await screen.findByTestId("revealed-secret");
    expect(textarea).toHaveValue("✓ à la mode");
    fireEvent.click(screen.getByRole("button", { name: "Copy to clipboard" }));
    await waitFor(() => expect(writeText).toHaveBeenCalledWith("✓ à la mode"));

    unmount();
    await waitFor(() =>
      expect(queryClient.getQueryData(queryKey)).toBeUndefined(),
    );
  });

  it("renders and copies an empty secret instead of treating it as missing", async () => {
    renderReveal("");
    await showSecret();

    expect(await screen.findByTestId("revealed-secret")).toHaveValue("");
    fireEvent.click(screen.getByRole("button", { name: "Copy to clipboard" }));
    await waitFor(() => expect(writeText).toHaveBeenCalledWith(""));
  });

  it("reports a missing secret separately", async () => {
    renderReveal(null);
    await showSecret();

    expect(
      await screen.findByText("The secret does not exist (or was removed)."),
    ).toBeInTheDocument();
    expect(screen.queryByTestId("revealed-secret")).not.toBeInTheDocument();
    expect(writeText).not.toHaveBeenCalled();
  });

  it("rejects malformed base64 without exposing it to the clipboard", async () => {
    renderReveal("%%%");
    await showSecret();

    expect(await screen.findByText(/returned malformed base64/i)).toBeInTheDocument();
    expect(screen.queryByTestId("revealed-secret")).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: /copy/i })).not.toBeInTheDocument();
    expect(writeText).not.toHaveBeenCalled();
  });

  it("rejects non-UTF-8 bytes without replacement text or clipboard access", async () => {
    renderReveal("//4=");
    await showSecret();

    expect(await screen.findByText(/contains non-UTF-8 bytes/i)).toBeInTheDocument();
    expect(screen.queryByText(/�/)).not.toBeInTheDocument();
    expect(screen.queryByTestId("revealed-secret")).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: /copy/i })).not.toBeInTheDocument();
    expect(writeText).not.toHaveBeenCalled();
  });
});

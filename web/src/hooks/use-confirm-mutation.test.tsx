import { readdirSync, readFileSync } from "node:fs";
import { dirname, relative, resolve } from "node:path";
import { fileURLToPath } from "node:url";

import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import {
  act,
  render,
  renderHook,
  screen,
  waitFor,
} from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type { ReactNode } from "react";
import { describe, expect, it, vi } from "vitest";

import { useConfirmMutation } from "./use-confirm-mutation";

function createHarness() {
  const queryClient = new QueryClient({
    defaultOptions: {
      mutations: { retry: false },
      queries: { retry: false },
    },
  });
  const wrapper = ({ children }: { children: ReactNode }) => (
    <QueryClientProvider client={queryClient}>{children}</QueryClientProvider>
  );
  return { queryClient, wrapper };
}

describe("useConfirmMutation", () => {
  it("returns the exact success result after invalidation and onSuccess", async () => {
    const resultValue = { id: "result" };
    const order: string[] = [];
    const mutationFn = vi.fn().mockResolvedValue(resultValue);
    const onSuccess = vi.fn(() => {
      order.push("success");
    });
    const { queryClient, wrapper } = createHarness();
    const invalidate = vi
      .spyOn(queryClient, "invalidateQueries")
      .mockImplementation(async (filters) => {
        order.push(`invalidate:${JSON.stringify(filters?.queryKey)}`);
      });
    const { result } = renderHook(
      () =>
        useConfirmMutation({
          mutationFn,
          invalidate: [["first"], ["second", 2]],
          onSuccess,
        }),
      { wrapper },
    );

    let outcome: Awaited<ReturnType<typeof result.current.confirm>> | undefined;
    await act(async () => {
      outcome = await result.current.confirm(undefined);
    });

    expect(outcome).toEqual({ status: "success", result: resultValue });
    expect(outcome?.status === "success" && outcome.result).toBe(resultValue);
    expect(invalidate).toHaveBeenCalledTimes(2);
    expect(invalidate).toHaveBeenNthCalledWith(1, { queryKey: ["first"] });
    expect(invalidate).toHaveBeenNthCalledWith(2, {
      queryKey: ["second", 2],
    });
    expect(order).toEqual([
      'invalidate:["first"]',
      'invalidate:["second",2]',
      "success",
    ]);
    expect(onSuccess).toHaveBeenCalledTimes(1);
    expect(result.current.submitError).toBeNull();
  });

  it.each([
    ["rejected promise", () => Promise.reject(new Error("request failed"))],
    [
      "synchronous throw",
      () => {
        throw new Error("request failed");
      },
    ],
  ])(
    "settles a raw mutation %s as an inline error outcome",
    async (_, failure) => {
      const mutationFn = vi.fn(failure);
      const onSuccess = vi.fn();
      const { queryClient, wrapper } = createHarness();
      const invalidate = vi.spyOn(queryClient, "invalidateQueries");
      const { result } = renderHook(
        () =>
          useConfirmMutation({
            mutationFn,
            invalidate: [["never"]],
            onSuccess,
          }),
        { wrapper },
      );

      let outcome:
        Awaited<ReturnType<typeof result.current.confirm>> | undefined;
      await act(async () => {
        outcome = await result.current.confirm(undefined);
      });

      expect(outcome).toEqual({ status: "error" });
      await waitFor(() =>
        expect(result.current.submitError).toBe(
          "Network error. Check your connection and try again.",
        ),
      );
      expect(invalidate).not.toHaveBeenCalled();
      expect(onSuccess).not.toHaveBeenCalled();
    },
  );

  it("clears an old inline error before a successful retry", async () => {
    const resultValue = { retried: true };
    let resolveRetry!: (value: typeof resultValue) => void;
    const pendingRetry = new Promise<typeof resultValue>((resolvePending) => {
      resolveRetry = resolvePending;
    });
    const mutationFn = vi
      .fn()
      .mockRejectedValueOnce(new Error("first failure"))
      .mockReturnValueOnce(pendingRetry);
    const { wrapper } = createHarness();
    const { result } = renderHook(() => useConfirmMutation({ mutationFn }), {
      wrapper,
    });

    await act(async () => {
      expect(await result.current.confirm(undefined)).toEqual({
        status: "error",
      });
    });
    expect(result.current.submitError).not.toBeNull();

    let retryPromise!: ReturnType<typeof result.current.confirm>;
    act(() => {
      retryPromise = result.current.confirm(undefined);
    });
    expect(result.current.submitError).toBeNull();

    let retry: Awaited<ReturnType<typeof result.current.confirm>> | undefined;
    await act(async () => {
      resolveRetry(resultValue);
      retry = await retryPromise;
    });

    expect(retry).toEqual({ status: "success", result: resultValue });
    expect(retry?.status === "success" && retry.result).toBe(resultValue);
    expect(result.current.submitError).toBeNull();
  });

  it("does not emit unhandledrejection when a request-error outcome is discarded", async () => {
    const collected: unknown[] = [];
    const listener = (event: PromiseRejectionEvent) => {
      collected.push(event.reason);
    };
    window.addEventListener("unhandledrejection", listener);
    try {
      const { wrapper } = createHarness();
      function Harness() {
        const { confirm, pending, submitError } = useConfirmMutation({
          mutationFn: () => Promise.reject(new Error("discarded failure")),
        });
        return (
          <>
            <button
              type="button"
              disabled={pending}
              onClick={() => void confirm(undefined)}
            >
              Confirm
            </button>
            {submitError ? <div role="alert">{submitError}</div> : null}
          </>
        );
      }
      const user = userEvent.setup();
      render(<Harness />, { wrapper });

      await user.click(screen.getByRole("button", { name: "Confirm" }));
      expect(await screen.findByRole("alert")).toHaveTextContent(
        "Network error. Check your connection and try again.",
      );
      await new Promise<void>((resolveMacrotask) => {
        setTimeout(resolveMacrotask, 0);
      });
      expect(collected).toEqual([]);
    } finally {
      window.removeEventListener("unhandledrejection", listener);
    }
  });

  it("leaves invalidation rejection on the mutateAsync error path", async () => {
    const failure = new Error("invalidation failed");
    const { queryClient, wrapper } = createHarness();
    vi.spyOn(queryClient, "invalidateQueries").mockRejectedValue(failure);
    const { result } = renderHook(
      () =>
        useConfirmMutation({
          mutationFn: async () => "result",
          invalidate: [["failing"]],
        }),
      { wrapper },
    );

    await expect(
      act(async () => result.current.confirm(undefined)),
    ).rejects.toBe(failure);
  });

  it("leaves opts.onSuccess rejection on the mutateAsync error path", async () => {
    const failure = new Error("success callback failed");
    const { wrapper } = createHarness();
    const { result } = renderHook(
      () =>
        useConfirmMutation({
          mutationFn: async () => "result",
          onSuccess: async () => {
            throw failure;
          },
        }),
      { wrapper },
    );

    await expect(
      act(async () => result.current.confirm(undefined)),
    ).rejects.toBe(failure);
  });
});

const SRC_ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "..");

function productionSources(directory = SRC_ROOT): Array<[string, string]> {
  const sources: Array<[string, string]> = [];
  for (const entry of readdirSync(directory, { withFileTypes: true })) {
    const path = resolve(directory, entry.name);
    if (entry.isDirectory()) {
      sources.push(...productionSources(path));
    } else if (/\.tsx?$/.test(entry.name) && !/\.test\./.test(entry.name)) {
      sources.push([
        relative(SRC_ROOT, path).replaceAll("\\", "/"),
        readFileSync(path, "utf8"),
      ]);
    }
  }
  return sources.sort(([left], [right]) => left.localeCompare(right));
}

function occurrenceCounts(
  sources: Array<[string, string]>,
  pattern: RegExp,
): Array<[string, number]> {
  return sources.flatMap(([path, source]) => {
    const flags = pattern.flags.includes("g")
      ? pattern.flags
      : `${pattern.flags}g`;
    const matches =
      source.match(new RegExp(pattern.source, flags))?.length ?? 0;
    return matches === 0 ? [] : [[path, matches]];
  });
}

function expectOneOccurrenceInEach(
  sources: Array<[string, string]>,
  pattern: RegExp,
  expectedPaths: string[],
) {
  expect(occurrenceCounts(sources, pattern)).toEqual(
    expectedPaths.map((path) => [path, 1]),
  );
}

function pathsMatching(
  sources: Array<[string, string]>,
  pattern: RegExp,
): string[] {
  return occurrenceCounts(sources, pattern).map(([path]) => path);
}

describe("confirmation mutation production inventory", () => {
  it("pins every hook, dialog, direct mutation, guard, and narrow catch path", () => {
    const sources = productionSources();
    expectOneOccurrenceInEach(
      sources,
      /(?<!function )\buseConfirmMutation(?:<[^>]+>)?\s*\(/,
      [
        "pages/client-instances/dialogs/delete.tsx",
        "pages/client-instances/dialogs/set-status.tsx",
        "pages/secrets/dialogs/delete-secret.tsx",
        "pages/secrets/libsecret/dialogs/delete-alias.tsx",
        "pages/secrets/libsecret/dialogs/delete-collection.tsx",
        "pages/secrets/libsecret/dialogs/delete-item.tsx",
        "pages/stores/dialogs/delete.tsx",
        "pages/stores/dialogs/lock.tsx",
        "pages/users/dialogs/delete.tsx",
      ],
    );

    expectOneOccurrenceInEach(sources, /<ConfirmDialog\b/, [
      "pages/client-instances/dialogs/delete.tsx",
      "pages/client-instances/dialogs/set-status.tsx",
      "pages/secrets/dialogs/delete-resource.tsx",
      "pages/secrets/dialogs/delete-secret.tsx",
      "pages/secrets/libsecret/dialogs/delete-alias.tsx",
      "pages/secrets/libsecret/dialogs/delete-collection.tsx",
      "pages/secrets/libsecret/dialogs/delete-item.tsx",
      "pages/stores/dialogs/delete.tsx",
      "pages/stores/dialogs/lock.tsx",
      "pages/users/dialogs/delete.tsx",
      "pages/wrapper-permissions/dialogs/delete-pair.tsx",
    ]);

    const mutateAsyncCalls = occurrenceCounts(sources, /\.mutateAsync\s*\(/);
    expect(mutateAsyncCalls).toEqual([
      ["hooks/use-confirm-mutation.ts", 1],
      ["pages/secrets/dialogs/delete-resource.tsx", 1],
      ["pages/wrapper-permissions/dialogs/delete-pair.tsx", 1],
    ]);
    expect(
      mutateAsyncCalls.reduce((total, [, count]) => total + count, 0),
    ).toBe(3);
    const directMutations = mutateAsyncCalls
      .filter(([path]) => path !== "hooks/use-confirm-mutation.ts")
      .map(([path]) => path);

    expectOneOccurrenceInEach(
      sources,
      /const outcome = await confirm\([\s\S]*?if \(outcome\.status === "error"\) return;[\s\S]*?const result = outcome\.result;/,
      ["pages/stores/dialogs/delete.tsx"],
    );

    expectOneOccurrenceInEach(
      sources,
      /const result = await postStoreRotateKey\(/,
      ["pages/stores/dialogs/rotate-key.tsx"],
    );
    const rotateSource = sources.find(
      ([path]) => path === "pages/stores/dialogs/rotate-key.tsx",
    )![1];
    expect(rotateSource).not.toMatch(/\buseMutation\b|\buseConfirmMutation\b/);

    const narrowCatchPattern =
      /try \{\s*result = await [^;]+\.mutateAsync\([^;]*\);\s*\} catch \{\s*return;\s*\}/;
    expectOneOccurrenceInEach(sources, narrowCatchPattern, directMutations);
    for (const path of directMutations) {
      const source = sources.find(([candidate]) => candidate === path)![1];
      const catchEnd =
        source.match(narrowCatchPattern)!.index! +
        source.match(narrowCatchPattern)![0].length;
      expect(source.indexOf("if (result.ok)")).toBeGreaterThan(catchEnd);
    }
  });

  it("pins the generic dialog and production global-rejection boundaries", () => {
    const sources = productionSources();
    const confirmDialog = sources.find(
      ([path]) => path === "components/confirm-dialog.tsx",
    )![1];
    expect(confirmDialog).not.toMatch(/\bcatch\b/);
    expect(
      pathsMatching(sources, /(?:unhandledrejection|onunhandledrejection)/i),
    ).toEqual([]);
    expect(
      pathsMatching(
        sources,
        /(?:unhandledrejection|onunhandledrejection)[\s\S]{0,200}preventDefault\(/i,
      ),
    ).toEqual([]);
  });
});

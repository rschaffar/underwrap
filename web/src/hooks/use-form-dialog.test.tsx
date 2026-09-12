import type { ReactNode } from "react";
import { act, renderHook } from "@testing-library/react";
import {
  QueryClient,
  QueryClientProvider,
  QueryObserver,
} from "@tanstack/react-query";
import { describe, expect, it, vi } from "vitest";
import { z } from "zod";

import { useFormDialog, type UseFormDialogOptions } from "./use-form-dialog";

type Values = { name: string };
type Result = { id: string };
type Options = UseFormDialogOptions<Values, Values, Result>;
function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((done) => {
    resolve = done;
  });
  return { promise, resolve };
}
function harness(overrides: Partial<Options> = {}) {
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false }, mutations: { retry: false } },
  });
  const submit = vi.fn(async (_values: Values) => ({ id: "saved-metadata" }));
  const onSuccess = vi.fn((_result: Result) => {});
  const options: Options = {
    schema: z.object({ name: z.string().min(1) }),
    defaultValues: { name: "Example metadata" },
    submit,
    onSuccess,
    invalidate: [["metadata"]],
    ...overrides,
  };
  const wrapper = ({ children }: { children: ReactNode }) => (
    <QueryClientProvider client={queryClient}>{children}</QueryClientProvider>
  );
  return {
    ...renderHook(() => useFormDialog(options), { wrapper }),
    queryClient,
    submit,
    onSuccess,
  };
}

describe("useFormDialog", () => {
  it("rejects a real active-query refresh failure without misreporting a saved request", async () => {
    const view = harness();
    const failure = new Error("refresh failed after saving");
    const queryKey = ["metadata"];
    view.queryClient.setQueryData(queryKey, "previous metadata");
    const queryFn = vi.fn().mockRejectedValue(failure);
    const observer = new QueryObserver(view.queryClient, {
      queryKey,
      queryFn,
      staleTime: Infinity,
      retry: false,
    });
    const unsubscribe = observer.subscribe(() => {});
    try {
      expect(queryFn).not.toHaveBeenCalled();
      await act(async () => {
        await expect(view.result.current.onSubmit()).rejects.toBe(failure);
      });
      expect(view.submit).toHaveBeenCalledOnce();
      expect(queryFn).toHaveBeenCalledOnce();
      expect(view.onSuccess).not.toHaveBeenCalled();
      expect(view.result.current.submitError).toBeUndefined();
      expect(view.result.current.pending).toBe(false);
      expect(view.queryClient.getQueryState(queryKey)?.status).toBe("error");
    } finally {
      unsubscribe();
      view.unmount();
      view.queryClient.clear();
    }
  });
  it("serializes validation, request, invalidation and callback, including same-turn submissions", async () => {
    const validation = deferred<void>();
    const request = deferred<Result>();
    const invalidation = deferred<void>();
    const callback = deferred<void>();
    const submit = vi.fn(() => request.promise);
    const onSuccess = vi.fn(() => callback.promise);
    const view = harness({
      schema: z.object({ name: z.string() }).superRefine(async () => {
        await validation.promise;
      }),
      submit,
      onSuccess,
    });
    vi.spyOn(view.queryClient, "invalidateQueries").mockReturnValue(
      invalidation.promise,
    );
    let first!: Promise<unknown>;
    let second!: Promise<unknown>;
    act(() => {
      first = view.result.current.onSubmit();
      second = view.result.current.onSubmit();
    });
    await act(async () => {
      await second;
    });
    expect(view.result.current.pending).toBe(true);
    expect(submit).not.toHaveBeenCalled();
    await act(async () => validation.resolve());
    expect(submit).toHaveBeenCalledTimes(1);
    await act(async () => request.resolve({ id: "saved-metadata" }));
    expect(view.result.current.pending).toBe(true);
    expect(onSuccess).not.toHaveBeenCalled();
    await act(async () => invalidation.resolve());
    expect(onSuccess).toHaveBeenCalledExactlyOnceWith({ id: "saved-metadata" });
    expect(view.result.current.pending).toBe(true);
    await act(async () => {
      callback.resolve();
      await first;
    });
    expect(view.result.current.pending).toBe(false);
    expect(view.result.current.submitError).toBeUndefined();
  });

  it("releases validation failures without a request and allows correction", async () => {
    const view = harness({ defaultValues: { name: "" } });
    await act(async () => {
      await view.result.current.onSubmit();
    });
    expect(view.submit).not.toHaveBeenCalled();
    expect(view.result.current.form.formState.errors.name).toBeDefined();
    expect(view.result.current.pending).toBe(false);
    act(() => view.result.current.form.setValue("name", "Corrected metadata"));
    await act(async () => {
      await view.result.current.onSubmit();
    });
    expect(view.submit).toHaveBeenCalledTimes(1);
    expect(view.onSuccess).toHaveBeenCalledTimes(1);
  });

  it("handles repeated identical request failures without leaving the guard stuck", async () => {
    const submit = vi
      .fn()
      .mockRejectedValue(new Error("synthetic request failure"));
    const view = harness({ submit });
    for (let attempt = 1; attempt <= 2; attempt++) {
      await act(async () => {
        await view.result.current.onSubmit();
      });
      expect(submit).toHaveBeenCalledTimes(attempt);
      expect(view.result.current.submitError).toMatch(/network error/i);
      expect(view.result.current.pending).toBe(false);
    }
    expect(view.onSuccess).not.toHaveBeenCalled();
  });

  it.each(["mapping", "invalidation", "callback"])(
    "keeps %s failure observable, not a network error",
    async (phase) => {
      const failure = new Error(`synthetic ${phase} failure`);
      const view = harness({
        ...(phase === "mapping"
          ? {
              toRequest: () => {
                throw failure;
              },
            }
          : {}),
        ...(phase === "callback"
          ? {
              onSuccess: async () => {
                throw failure;
              },
            }
          : {}),
      });
      if (phase === "invalidation")
        vi.spyOn(view.queryClient, "invalidateQueries").mockRejectedValue(
          failure,
        );
      await act(async () => {
        await expect(view.result.current.onSubmit()).rejects.toBe(failure);
      });
      expect(view.result.current.pending).toBe(false);
      expect(view.result.current.submitError).toBeUndefined();
      expect(view.submit).toHaveBeenCalledTimes(phase === "mapping" ? 0 : 1);
      expect(view.onSuccess).not.toHaveBeenCalled();
    },
  );
});

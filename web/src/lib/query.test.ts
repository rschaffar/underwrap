import { QueryClient, QueryObserver } from "@tanstack/react-query";
import { describe, expect, it, vi } from "vitest";
import { invalidateAll, invalidateAllInBackground } from "./query";

function activeFailingQuery() {
  const client = new QueryClient({
    defaultOptions: { queries: { retry: false, gcTime: Infinity } },
  });
  const key = ["metadata"];
  const failure = new Error("active metadata refresh failed");
  client.setQueryData(key, "cached metadata");
  const queryFn = vi.fn().mockRejectedValue(failure);
  const observer = new QueryObserver(client, {
    queryKey: key,
    queryFn,
    staleTime: Infinity,
  });
  const unsubscribe = observer.subscribe(() => {});
  return {
    client,
    key,
    failure,
    queryFn,
    dispose() {
      unsubscribe();
      client.clear();
    },
  };
}

describe("query invalidation ownership", () => {
  it("rejects awaited active refresh failure while retaining query error state", async () => {
    const view = activeFailingQuery();
    try {
      expect(view.queryFn).not.toHaveBeenCalled();
      await expect(invalidateAll(view.client, [view.key])).rejects.toBe(
        view.failure,
      );
      expect(view.queryFn).toHaveBeenCalledOnce();
      expect(view.client.getQueryState(view.key)?.error).toBe(view.failure);
    } finally {
      view.dispose();
    }
  });

  it("keeps explicitly background refresh non-blocking without hiding query error state", async () => {
    const view = activeFailingQuery();
    try {
      expect(
        invalidateAllInBackground(view.client, [view.key]),
      ).toBeUndefined();
      await vi.waitFor(() =>
        expect(view.client.getQueryState(view.key)?.status).toBe("error"),
      );
      expect(view.queryFn).toHaveBeenCalledOnce();
      expect(view.client.getQueryState(view.key)?.error).toBe(view.failure);
    } finally {
      view.dispose();
    }
  });
});

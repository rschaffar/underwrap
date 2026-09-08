import type { QueryClient } from "@tanstack/react-query";

/** Invalidate every listed query key in parallel. */
export async function invalidateAll(
  qc: QueryClient,
  keys: ReadonlyArray<readonly unknown[]>,
): Promise<void> {
  await Promise.all(keys.map((key) => qc.invalidateQueries({ queryKey: key })));
}

/** Launch invalidation without extending a mutation's pending/error lifecycle. */
export function invalidateAllInBackground(
  qc: QueryClient,
  keys: ReadonlyArray<readonly unknown[]>,
): void {
  void invalidateAll(qc, keys).catch(() => {
    // A later query refresh can reconcile the current server state.
  });
}

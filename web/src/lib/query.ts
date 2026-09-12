import type { QueryClient } from "@tanstack/react-query";

/** Await invalidation and active refreshes; preserve refresh rejection. */
export async function invalidateAll(
  qc: QueryClient,
  keys: ReadonlyArray<readonly unknown[]>,
): Promise<void> {
  await Promise.all(
    keys.map((key) =>
      qc.invalidateQueries({ queryKey: key }, { throwOnError: true }),
    ),
  );
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

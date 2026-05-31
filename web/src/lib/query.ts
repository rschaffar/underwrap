import type { QueryClient } from "@tanstack/react-query";

/** Invalidate every listed query key in parallel. */
export async function invalidateAll(
  qc: QueryClient,
  keys: ReadonlyArray<readonly unknown[]>,
): Promise<void> {
  await Promise.all(keys.map((key) => qc.invalidateQueries({ queryKey: key })));
}

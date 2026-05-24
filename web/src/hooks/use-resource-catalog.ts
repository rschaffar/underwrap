import { useMemo } from "react";

import type { ResourceRecord } from "@/api/client";

/**
 * Build per-resource lookup maps from a store's resource list. Both
 * the Secrets and Wrapper-permissions pages need `id → name` (for
 * label rendering) and `id → record` (for provenance), built off
 * the same `resources` array that arrives in their per-store
 * aggregate views.
 */
export function useResourceCatalog(
  resources: ReadonlyArray<ResourceRecord> | undefined,
): {
  nameById: Map<string, string>;
  recordById: Map<string, ResourceRecord>;
} {
  return useMemo(() => {
    const nameById = new Map<string, string>();
    const recordById = new Map<string, ResourceRecord>();
    for (const r of resources ?? []) {
      nameById.set(r.id, r.name);
      recordById.set(r.id, r);
    }
    return { nameById, recordById };
  }, [resources]);
}

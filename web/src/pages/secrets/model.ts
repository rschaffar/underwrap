import type { AdminSecretEntry, ResourceSlot } from "@/api/client";

/** The three backend slots every resource has. */
export const SECRET_SLOTS = ["env", "files", "libsecret"] as const satisfies ReadonlyArray<ResourceSlot>;
export type SecretSlot = ResourceSlot;

/** A single per-slot entry inside a resource. */
export interface SecretEntry {
  slot: SecretSlot;
  key: string;
}

/** Address of a per-slot entry — the structured tuple the admin API takes. */
export interface SecretAddress {
  resource_id: string;
  slot: SecretSlot;
  key: string;
}

export interface SecretResourceGroup {
  /** Stable resource id; the operator-facing label is resolved via the catalog. */
  resource_id: string;
  env: SecretEntry[];
  files: SecretEntry[];
  libsecret: SecretEntry[];
}

/** Total entry count across all three slots of a resource. */
export function resourceCount(g: SecretResourceGroup): number {
  return g.env.length + g.files.length + g.libsecret.length;
}

/**
 * Group structured `AdminSecretEntry` rows from the admin API into
 * per-resource buckets, with each slot sorted by key. The backend
 * already guarantees the shape (legacy path-encoded ids never reach
 * the wire), so there is no "unrecognized" bucket here.
 */
export function groupSecretEntries(
  entries: AdminSecretEntry[],
): SecretResourceGroup[] {
  const groups = new Map<string, SecretResourceGroup>();
  for (const entry of entries) {
    let g = groups.get(entry.resource_id);
    if (!g) {
      g = { resource_id: entry.resource_id, env: [], files: [], libsecret: [] };
      groups.set(entry.resource_id, g);
    }
    g[entry.slot].push({ slot: entry.slot, key: entry.key });
  }
  for (const g of groups.values()) {
    for (const slot of SECRET_SLOTS) {
      g[slot].sort((a, b) => a.key.localeCompare(b.key));
    }
  }
  return [...groups.values()].sort((a, b) =>
    a.resource_id.localeCompare(b.resource_id),
  );
}

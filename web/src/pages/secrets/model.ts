import type { AdminSecretEntry, ResourceSlot } from "@/api/client";

/**
 * The slots this page renders as flat key/value rows. The third slot
 * the backend defines (`libsecret`) is structurally a tree and is
 * handled separately by `LibsecretTree`; it never reaches the model
 * here.
 */
export const SECRET_SLOTS = ["env", "files"] as const satisfies ReadonlyArray<ResourceSlot>;
export type SecretSlot = (typeof SECRET_SLOTS)[number];

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
}

/** Total entry count across the flat slots of a resource. */
export function resourceCount(g: SecretResourceGroup): number {
  return g.env.length + g.files.length;
}

/**
 * Group structured `AdminSecretEntry` rows from the admin API into
 * per-resource buckets, with each slot sorted by key. Libsecret
 * entries are filtered out defensively — the backend doesn't emit
 * them through this endpoint after the flat-KV bridge was retired,
 * but a stale client cache could still hand us one.
 */
export function groupSecretEntries(
  entries: AdminSecretEntry[],
): SecretResourceGroup[] {
  const groups = new Map<string, SecretResourceGroup>();
  for (const entry of entries) {
    if (entry.slot !== "env" && entry.slot !== "files") {
      continue;
    }
    let g = groups.get(entry.resource_id);
    if (!g) {
      g = { resource_id: entry.resource_id, env: [], files: [] };
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

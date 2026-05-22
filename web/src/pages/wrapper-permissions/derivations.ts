import type {
  WrapperApprovalMode,
  WrapperPermission,
  WrapperResourcePairRecord,
} from "@/api/client";

/** Canonical kebab string for a permission mode. */
export type ModeKind = "locked" | "unlocked" | "unlocked-until";

/** `WrapperApprovalMode` is a tagged union over `kind`; this maps to it. */
export function modeKind(mode: WrapperApprovalMode): ModeKind {
  return mode.kind;
}

/** A `(wrapper, resource_id)` pair — one row in the table. */
export interface WrapperResource {
  wrapper: string;
  resource_id: string;
}

export function wrapperResourceKey(wr: WrapperResource): string {
  return `${wr.wrapper}|${wr.resource_id}`;
}

/** Collect every distinct runtime client instance mentioned in permissions. */
export function deriveClientInstances(permissions: WrapperPermission[]): string[] {
  const set = new Set<string>();
  for (const p of permissions) set.add(p.client_instance);
  return [...set].sort();
}

/**
 * Compute the set of client-instance columns for the table.
 *
 * Primary source is the list of client instances currently bound to
 * `storeId` — so an admin can grant permissions to any connected
 * instance even before it has any rows. Permissions whose
 * `client_instance` is not in that set (stale rows from a previous
 * store binding, etc.) are still surfaced so they remain visible and
 * removable.
 */
export function clientInstancesForStore(
  storeId: string,
  instances: ReadonlyArray<{ id: string; current_store_id: string }>,
  permissions: WrapperPermission[],
): string[] {
  const set = new Set<string>();
  for (const i of instances) {
    if (i.current_store_id === storeId) set.add(i.id);
  }
  for (const p of permissions) set.add(p.client_instance);
  return [...set].sort();
}

/** Sort pair records into the (wrapper, resource_id) order the table renders. */
export function sortPairs(
  pairs: WrapperResourcePairRecord[],
): WrapperResourcePairRecord[] {
  return [...pairs].sort((a, b) =>
    a.wrapper === b.wrapper
      ? a.resource_id.localeCompare(b.resource_id)
      : a.wrapper.localeCompare(b.wrapper),
  );
}

/**
 * Group pair records by resource, sorted by resource name, with each
 * group's wrappers sorted by wrapper name. The page renders one table
 * per group, with the resource shown as the section heading.
 */
export function groupPairsByResource(
  pairs: WrapperResourcePairRecord[],
): { resource: string; pairs: WrapperResourcePairRecord[] }[] {
  const buckets = new Map<string, WrapperResourcePairRecord[]>();
  for (const p of pairs) {
    const arr = buckets.get(p.resource_id);
    if (arr) arr.push(p);
    else buckets.set(p.resource_id, [p]);
  }
  return [...buckets.entries()]
    .map(([resource, group]) => ({
      resource,
      pairs: [...group].sort((a, b) => a.wrapper.localeCompare(b.wrapper)),
    }))
    .sort((a, b) => a.resource.localeCompare(b.resource));
}

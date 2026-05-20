import { useEffect, useMemo, useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import {
  ShieldCheck,
  ShieldOff,
  Hourglass,
  KeyRound,
  Plus,
  FileText,
  Trash2,
} from "lucide-react";

import { Button } from "@/components/ui/button";
import { PolicyViewerDialog } from "@/components/policy-viewer-dialog";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import {
  type AdminClientInstancesListResponse,
  type AdminPairsListResponse,
  type AdminResourcesListResponse,
  type AdminStoresListResponse,
  type AdminWrapperPermissionsListResponse,
  type EntityCreator,
  fetchClientInstances,
  fetchStores,
  postPairCreate,
  postPairDelete,
  postPairsList,
  postResourceCreate,
  postResourcesList,
  postWrapperPermissionDelete,
  postWrapperPermissionUpsert,
  postWrapperPermissionsList,
  type ResourceRecord,
  type WhoamiResponse,
  type WrapperApprovalMode,
  type WrapperPermission,
  type WrapperResourcePairRecord,
} from "@/api/client";
import { describeApiError } from "@/lib/error-message";

interface WrapperPermissionsPageProps {
  principal: WhoamiResponse | null;
}

/** Canonical kebab string for a permission mode. */
export type ModeKind =
  | "locked"
  | "unlocked"
  | "unlocked-until"
  | "per-call-approval";

/** `WrapperApprovalMode` is a tagged union over `kind`; this maps to it. */
export function modeKind(mode: WrapperApprovalMode): ModeKind {
  return mode.kind as ModeKind;
}

/** A `(wrapper, resource)` pair — one row in the table. */
export interface WrapperResource {
  wrapper: string;
  resource: string;
}

export function wrapperResourceKey(wr: WrapperResource): string {
  return `${wr.wrapper}|${wr.resource}`;
}

/** Format a u64 epoch as a relative time label ("3 min ago"). */
export function formatRelativeFromEpoch(
  epoch: bigint | number | null | undefined,
  now: number = Date.now(),
): string {
  if (epoch === null || epoch === undefined) return "never";
  const n = typeof epoch === "bigint" ? Number(epoch) : epoch;
  if (!n) return "never";
  const diffSecs = Math.round(now / 1000 - n);
  const rtf = new Intl.RelativeTimeFormat(undefined, { numeric: "auto" });
  const abs = Math.abs(diffSecs);
  if (abs < 60) return rtf.format(-diffSecs, "second");
  if (abs < 3600) return rtf.format(-Math.round(diffSecs / 60), "minute");
  if (abs < 86400) return rtf.format(-Math.round(diffSecs / 3600), "hour");
  return rtf.format(-Math.round(diffSecs / 86400), "day");
}

/** Format a u64 epoch as a full local ISO-style timestamp for tooltips. */
function formatEpochFull(epoch: bigint | number | null | undefined): string {
  if (epoch === null || epoch === undefined) return "never";
  const n = typeof epoch === "bigint" ? Number(epoch) : epoch;
  if (!n) return "never";
  return new Date(n * 1000).toLocaleString();
}

/**
 * Render an `EntityCreator` as a short human-readable label. When the
 * creator is a runtime client instance, prefer the operator-facing
 * display name resolved from `displayNameById` over the opaque id.
 */
function describeCreator(
  creator: EntityCreator,
  displayNameById?: Map<string, string>,
): string {
  switch (creator.kind) {
    case "operator":
      return creator.username;
    case "client-instance":
      return `client ${displayNameById?.get(creator.id) ?? creator.id}`;
  }
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

/** Sort pair records into the (wrapper, resource) order the table renders. */
export function sortPairs(
  pairs: WrapperResourcePairRecord[],
): WrapperResourcePairRecord[] {
  return [...pairs].sort((a, b) =>
    a.wrapper === b.wrapper
      ? a.resource.localeCompare(b.resource)
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
    const arr = buckets.get(p.resource);
    if (arr) arr.push(p);
    else buckets.set(p.resource, [p]);
  }
  return [...buckets.entries()]
    .map(([resource, group]) => ({
      resource,
      pairs: [...group].sort((a, b) => a.wrapper.localeCompare(b.wrapper)),
    }))
    .sort((a, b) => a.resource.localeCompare(b.resource));
}

export function WrapperPermissionsPage({ principal }: WrapperPermissionsPageProps) {
  useEffect(() => {
    document.title = "underwrap — wrapper permissions";
  }, []);

  const isAdmin = !!(principal?.authenticated && principal.role === "admin");
  const qc = useQueryClient();

  const stores = useQuery<AdminStoresListResponse>({
    queryKey: ["admin", "stores"],
    queryFn: fetchStores,
  });
  const initialStore = (() => {
    if (typeof window === "undefined") return null;
    return new URLSearchParams(window.location.search).get("store");
  })();
  const [storeName, setStoreName] = useState<string | null>(initialStore);

  useEffect(() => {
    if (storeName) return;
    const first = stores.data?.items[0]?.id;
    if (first) setStoreName(first);
  }, [storeName, stores.data]);

  useEffect(() => {
    if (!storeName) return;
    const url = new URL(window.location.href);
    url.searchParams.set("store", storeName);
    window.history.replaceState({}, "", url.toString());
  }, [storeName]);

  const permissions = useQuery<AdminWrapperPermissionsListResponse>({
    queryKey: ["admin", "wrapper-permissions", storeName],
    queryFn: () => postWrapperPermissionsList({ store: storeName! }),
    enabled: !!storeName,
  });
  const pairs = useQuery<AdminPairsListResponse>({
    queryKey: ["admin", "pairs", storeName],
    queryFn: () => postPairsList({ store: storeName! }),
    enabled: !!storeName,
  });
  const resources = useQuery<AdminResourcesListResponse>({
    queryKey: ["admin", "resources", storeName],
    queryFn: () => postResourcesList({ store: storeName! }),
    enabled: !!storeName,
  });
  // The pairs and wrapper permissions reference client instances by
  // id; the operator-facing column header shows the editable display
  // name resolved from this list.
  const clientInstancesList = useQuery<AdminClientInstancesListResponse>({
    queryKey: ["admin", "client-instances"],
    queryFn: fetchClientInstances,
  });
  const displayNameById = useMemo(() => {
    const map = new Map<string, string>();
    for (const item of clientInstancesList.data?.items ?? []) {
      map.set(item.id, item.display_name);
    }
    return map;
  }, [clientInstancesList.data]);
  const pairRecords = useMemo(
    () => sortPairs(pairs.data?.pairs ?? []),
    [pairs.data],
  );
  const clientInstances = useMemo(
    () =>
      storeName
        ? clientInstancesForStore(
            storeName,
            clientInstancesList.data?.items ?? [],
            permissions.data?.permissions ?? [],
          )
        : deriveClientInstances(permissions.data?.permissions ?? []),
    [storeName, clientInstancesList.data, permissions.data],
  );

  /** Map `(client_instance, wrapper, resource)` → permission. */
  const permissionIndex = useMemo(() => {
    const map = new Map<string, WrapperPermission>();
    for (const p of permissions.data?.permissions ?? []) {
      map.set(`${p.client_instance}|${p.wrapper}|${p.resource}`, p);
    }
    return map;
  }, [permissions.data]);

  const [editing, setEditing] = useState<{
    clientInstance: string;
    wrapper: string;
    resource: string;
    permission: WrapperPermission | null;
  } | null>(null);
  const [rawOpen, setRawOpen] = useState(false);
  const [creatingPair, setCreatingPair] = useState(false);
  const [deletingPair, setDeletingPair] =
    useState<WrapperResourcePairRecord | null>(null);

  function invalidatePairsAndPermissions() {
    if (!storeName) return;
    qc.invalidateQueries({ queryKey: ["admin", "pairs", storeName] });
    qc.invalidateQueries({ queryKey: ["admin", "wrapper-permissions", storeName] });
  }

  if (!storeName) {
    return (
      <div className="space-y-4">
        <h2 className="text-xl font-semibold text-(--color-fg)">
          Wrapper permissions
        </h2>
        {stores.isLoading ? (
          <p className="text-sm text-(--color-muted)">Loading…</p>
        ) : (
          <p
            role="alert"
            className="rounded-md border border-amber-500/30 bg-amber-500/10 px-3 py-2 text-sm text-amber-200"
          >
            No stores are visible to you. Ask an admin for access to
            at least one store.
          </p>
        )}
      </div>
    );
  }

  return (
    <div className="space-y-6">
      <header className="flex flex-wrap items-baseline justify-between gap-3">
        <div>
          <h2 className="text-xl font-semibold text-(--color-fg)">
            Wrapper permissions
          </h2>
          <p className="mt-1 text-sm text-(--color-muted)">
            Wrapper/resource pairs registered with the backend, and the
            permission each runtime client instance holds against them.
            Click a cell to set or change the mode.
          </p>
        </div>
        <div className="flex items-center gap-3">
          {isAdmin ? (
            <Button
              type="button"
              size="sm"
              onClick={() => setCreatingPair(true)}
              data-testid="wrappers-create-pair"
            >
              <Plus className="mr-1.5 h-4 w-4" aria-hidden />
              Create pair
            </Button>
          ) : null}
          <Button
            type="button"
            variant="ghost"
            size="sm"
            onClick={() => setRawOpen(true)}
          >
            <FileText className="mr-1.5 h-4 w-4" aria-hidden />
            View raw policy
          </Button>
          <label className="text-sm text-(--color-muted)">
            Store:{" "}
            <select
              data-testid="wrappers-store-select"
              value={storeName}
              onChange={(e) => setStoreName(e.target.value)}
              className="ml-1 rounded-md border border-(--color-line) bg-(--color-surface-0) px-2 py-1 text-sm text-(--color-fg)"
            >
              {(stores.data?.items ?? []).map((s) => (
                <option key={s.id} value={s.id}>
                  {s.name}
                </option>
              ))}
            </select>
          </label>
        </div>
      </header>

      {permissions.isLoading || pairs.isLoading ? (
        <p className="text-sm text-(--color-muted)">
          Loading wrapper permissions…
        </p>
      ) : permissions.error || pairs.error ? (
        <p role="alert" className="text-sm text-rose-300">
          {describeApiError(permissions.error ?? pairs.error)}
        </p>
      ) : pairRecords.length === 0 ? (
        <p className="text-sm text-(--color-muted)">
          No wrapper/resource pairs are registered for this store yet.
          {isAdmin
            ? " Use Create pair to add one, or enable auto-create on a client instance so its wrappers register on startup."
            : " Ask an admin to add one, or enable auto-create for a client instance."}
        </p>
      ) : (
        <ResourceGroupedPairsTable
          groups={groupPairsByResource(pairRecords)}
          clientInstances={clientInstances}
          displayNameById={displayNameById}
          permissionIndex={permissionIndex}
          isAdmin={isAdmin}
          onCellClick={(wr, clientInstance, permission) =>
            setEditing({
              clientInstance,
              wrapper: wr.wrapper,
              resource: wr.resource,
              permission,
            })
          }
          onDeletePair={(pair) => setDeletingPair(pair)}
        />
      )}

      {editing ? (
        <EditCellDialog
          storeName={storeName}
          clientInstance={editing.clientInstance}
          clientInstanceDisplayName={
            displayNameById.get(editing.clientInstance) ?? editing.clientInstance
          }
          wrapper={editing.wrapper}
          resource={editing.resource}
          existing={editing.permission}
          canEdit={isAdmin}
          onClose={() => setEditing(null)}
        />
      ) : null}

      {creatingPair ? (
        <CreatePairDialog
          storeName={storeName}
          existingResources={resources.data?.resources ?? []}
          existingPairs={pairRecords}
          onClose={() => setCreatingPair(false)}
          onCreated={() => {
            invalidatePairsAndPermissions();
            qc.invalidateQueries({ queryKey: ["admin", "resources", storeName] });
            setCreatingPair(false);
          }}
        />
      ) : null}

      {deletingPair ? (
        <DeletePairDialog
          storeName={storeName}
          pair={deletingPair}
          displayNameById={displayNameById}
          onClose={() => setDeletingPair(null)}
          onDeleted={() => {
            invalidatePairsAndPermissions();
            setDeletingPair(null);
          }}
        />
      ) : null}

      <PolicyViewerDialog
        storeName={storeName}
        open={rawOpen}
        onClose={() => setRawOpen(false)}
      />
    </div>
  );
}

// ---------- Subcomponents ----------

function ResourceGroupedPairsTable({
  groups,
  clientInstances,
  displayNameById,
  permissionIndex,
  isAdmin,
  onCellClick,
  onDeletePair,
}: {
  groups: { resource: string; pairs: WrapperResourcePairRecord[] }[];
  clientInstances: string[];
  displayNameById: Map<string, string>;
  permissionIndex: Map<string, WrapperPermission>;
  isAdmin: boolean;
  onCellClick: (
    wr: WrapperResource,
    clientInstance: string,
    permission: WrapperPermission | null,
  ) => void;
  onDeletePair: (pair: WrapperResourcePairRecord) => void;
}) {
  return (
    <div className="overflow-x-auto rounded-xl border border-(--color-line)">
      <table className="min-w-full text-left text-sm">
        <thead className="bg-(--color-surface-0)/60 text-xs uppercase tracking-wide text-(--color-muted)">
          <tr>
            <th className="sticky left-0 z-10 bg-(--color-surface-0)/60 px-4 py-2">
              Resource
            </th>
            <th className="bg-(--color-surface-0)/60 px-4 py-2">Wrapper</th>
            <th className="bg-(--color-surface-0)/60 px-4 py-2 text-(--color-muted)">
              Last used
            </th>
            {clientInstances.map((c) => {
              const displayName = displayNameById.get(c) ?? c;
              return (
                <th key={c} className="px-3 py-2" title={c}>
                  <div className="font-medium text-(--color-fg)">
                    {displayName}
                  </div>
                </th>
              );
            })}
            {isAdmin ? <th className="px-3 py-2" /> : null}
          </tr>
        </thead>
        <tbody>
          {groups.flatMap((group) =>
            group.pairs.map((pair, indexWithinGroup) => {
              const wr: WrapperResource = {
                wrapper: pair.wrapper,
                resource: pair.resource,
              };
              const provenance = `Created ${formatEpochFull(
                pair.created_at,
              )} by ${describeCreator(pair.created_by, displayNameById)}`;
              const isFirstInGroup = indexWithinGroup === 0;
              return (
                <tr
                  key={wrapperResourceKey(wr)}
                  className={
                    "align-middle " +
                    (isFirstInGroup ? "border-t-2 border-(--color-line)" : "")
                  }
                >
                  {isFirstInGroup ? (
                    <th
                      scope="rowgroup"
                      rowSpan={group.pairs.length}
                      className="sticky left-0 z-10 bg-(--color-surface-0)/40 px-4 py-3 text-left align-top font-medium text-(--color-fg)"
                      data-testid={`wrappers-resource-${group.resource}`}
                    >
                      <code className="rounded bg-(--color-surface-1) px-1.5 py-0.5 text-xs">
                        {group.resource}
                      </code>
                    </th>
                  ) : null}
                  <td
                    className="px-4 py-3 font-medium text-(--color-fg)"
                    title={provenance}
                    data-testid={`wrappers-provenance-${pair.wrapper}-${pair.resource}`}
                  >
                    {pair.wrapper}
                  </td>
                  <td
                    className="px-4 py-3 text-xs text-(--color-muted)"
                    title={`Last used: ${formatEpochFull(pair.last_used_at)}`}
                    data-testid={`wrappers-last-used-${pair.wrapper}-${pair.resource}`}
                  >
                    {pair.last_used_at == null ? (
                      <span className="opacity-60">never used</span>
                    ) : (
                      <>used {formatRelativeFromEpoch(pair.last_used_at)}</>
                    )}
                  </td>
                  {clientInstances.map((c) => {
                    const key = `${c}|${pair.wrapper}|${pair.resource}`;
                    const permission = permissionIndex.get(key) ?? null;
                    return (
                      <td key={key} className="px-3 py-3">
                        <button
                          type="button"
                          data-testid={`wrappers-cell-${pair.wrapper}-${pair.resource}-${c}`}
                          onClick={() => onCellClick(wr, c, permission)}
                          disabled={!isAdmin}
                          className="min-h-11 min-w-[5rem] rounded-md border border-(--color-line) bg-(--color-surface-1) px-2 py-1 text-xs hover:bg-(--color-surface-0) disabled:opacity-60"
                        >
                          <CellContent permission={permission} />
                        </button>
                      </td>
                    );
                  })}
                  {isAdmin ? (
                    <td className="px-3 py-3">
                      <Button
                        type="button"
                        variant="ghost"
                        size="sm"
                        aria-label={`Delete pair ${pair.wrapper} / ${pair.resource}`}
                        data-testid={`wrappers-delete-pair-${pair.wrapper}-${pair.resource}`}
                        onClick={() => onDeletePair(pair)}
                      >
                        <Trash2 className="h-4 w-4" aria-hidden />
                      </Button>
                    </td>
                  ) : null}
                </tr>
              );
            }),
          )}
        </tbody>
      </table>
    </div>
  );
}

/**
 * Absent permissions render as Locked — the same badge the explicit
 * `Locked` mode uses. This collapses two "deny" states (no row vs. a
 * row pinned to Locked) into a single visual: from the operator's
 * perspective, both deny reads, and the dialog still lets them upgrade
 * to a different mode.
 */
function CellContent({ permission }: { permission: WrapperPermission | null }) {
  if (!permission) {
    return <ModeBadge mode={{ kind: "locked" } as WrapperApprovalMode} />;
  }
  return <ModeBadge mode={permission.mode} />;
}

export function ModeBadge({ mode }: { mode: WrapperApprovalMode }) {
  switch (mode.kind) {
    case "locked":
      return (
        <span
          data-testid="mode-locked"
          className="inline-flex items-center gap-1 rounded-md border border-rose-500/30 bg-rose-500/10 px-1.5 py-0.5 text-xs text-rose-200"
        >
          <ShieldOff className="h-3 w-3" aria-hidden /> locked
        </span>
      );
    case "unlocked":
      return (
        <span
          data-testid="mode-unlocked"
          className="inline-flex items-center gap-1 rounded-md border border-emerald-500/30 bg-emerald-500/10 px-1.5 py-0.5 text-xs text-emerald-200"
        >
          <ShieldCheck className="h-3 w-3" aria-hidden /> unlocked
        </span>
      );
    case "unlocked-until": {
      const until = Number((mode as { until_epoch: bigint | number }).until_epoch);
      const date = new Date(until * 1000);
      return (
        <span
          data-testid="mode-unlocked-until"
          className="inline-flex items-center gap-1 rounded-md border border-amber-500/30 bg-amber-500/10 px-1.5 py-0.5 text-xs text-amber-200"
          title={`Expires ${date.toString()}`}
        >
          <Hourglass className="h-3 w-3" aria-hidden /> until {date.toLocaleDateString()}
        </span>
      );
    }
    case "per-call-approval":
      return (
        <span
          data-testid="mode-per-call-approval"
          className="inline-flex items-center gap-1 rounded-md border border-blue-500/30 bg-blue-500/10 px-1.5 py-0.5 text-xs text-blue-200"
        >
          <KeyRound className="h-3 w-3" aria-hidden /> per-call
        </span>
      );
  }
}

// ---------- Create pair ----------

function CreatePairDialog({
  storeName,
  existingResources,
  existingPairs,
  onClose,
  onCreated,
}: {
  storeName: string;
  existingResources: ResourceRecord[];
  existingPairs: WrapperResourcePairRecord[];
  onClose: () => void;
  onCreated: () => void;
}) {
  const [wrapper, setWrapper] = useState("");
  const [resource, setResource] = useState<string>(
    existingResources[0]?.name ?? "",
  );
  const [creatingNewResource, setCreatingNewResource] = useState(
    existingResources.length === 0,
  );
  const [newResourceName, setNewResourceName] = useState("");
  const [submitError, setSubmitError] = useState<string | null>(null);

  const createResource = useMutation({
    mutationFn: (name: string) => postResourceCreate({ store: storeName, name }),
  });
  const createPair = useMutation({
    mutationFn: (vars: { wrapper: string; resource: string }) =>
      postPairCreate({
        store: storeName,
        wrapper: vars.wrapper,
        resource: vars.resource,
      }),
  });

  async function onSave() {
    setSubmitError(null);
    const w = wrapper.trim();
    if (!w) {
      setSubmitError("Wrapper name is required.");
      return;
    }
    let chosenResource = resource;
    if (creatingNewResource) {
      const r = newResourceName.trim();
      if (!r) {
        setSubmitError("Resource name is required.");
        return;
      }
      try {
        const created = await createResource.mutateAsync(r);
        chosenResource = created.resource.name;
      } catch (e) {
        setSubmitError(describeApiError(e));
        return;
      }
    }
    if (!chosenResource) {
      setSubmitError("Pick or create a resource.");
      return;
    }
    if (
      existingPairs.some(
        (p) => p.wrapper === w && p.resource === chosenResource,
      )
    ) {
      setSubmitError(`Pair (${w}, ${chosenResource}) already exists.`);
      return;
    }
    try {
      await createPair.mutateAsync({ wrapper: w, resource: chosenResource });
      onCreated();
    } catch (e) {
      setSubmitError(describeApiError(e));
    }
  }

  const pending = createResource.isPending || createPair.isPending;

  return (
    <Dialog open onOpenChange={(o) => !o && onClose()}>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>Create wrapper/resource pair</DialogTitle>
          <DialogDescription>
            A wrapper/resource pair is the unit wrapper permissions
            reference. Each registered pair becomes a row in the table.
          </DialogDescription>
        </DialogHeader>
        <div className="space-y-3">
          <div className="space-y-1.5">
            <Label htmlFor="pair-wrapper">Wrapper name</Label>
            <Input
              id="pair-wrapper"
              value={wrapper}
              onChange={(e) => setWrapper(e.target.value)}
              placeholder="e.g. gh-authorized"
              autoFocus
              autoComplete="off"
            />
          </div>
          <div className="space-y-1.5">
            <Label>Resource</Label>
            {existingResources.length > 0 ? (
              <div className="flex items-center gap-2 text-sm">
                <label className="inline-flex items-center gap-1.5">
                  <input
                    type="radio"
                    name="resource-mode"
                    checked={!creatingNewResource}
                    onChange={() => setCreatingNewResource(false)}
                  />
                  Pick existing
                </label>
                <label className="inline-flex items-center gap-1.5">
                  <input
                    type="radio"
                    name="resource-mode"
                    checked={creatingNewResource}
                    onChange={() => setCreatingNewResource(true)}
                  />
                  Create new
                </label>
              </div>
            ) : null}
            {creatingNewResource ? (
              <Input
                data-testid="pair-new-resource"
                value={newResourceName}
                onChange={(e) => setNewResourceName(e.target.value)}
                placeholder="e.g. github/token"
                autoComplete="off"
              />
            ) : (
              <select
                data-testid="pair-resource-select"
                value={resource}
                onChange={(e) => setResource(e.target.value)}
                className="block w-full rounded-md border border-(--color-line) bg-(--color-surface-0) px-2 py-1.5 text-sm text-(--color-fg)"
              >
                {existingResources.map((r) => (
                  <option key={r.name} value={r.name}>
                    {r.name}
                  </option>
                ))}
              </select>
            )}
          </div>
          {submitError ? (
            <p
              role="alert"
              className="rounded-md border border-rose-500/30 bg-rose-500/10 px-3 py-2 text-xs text-rose-300"
            >
              {submitError}
            </p>
          ) : null}
        </div>
        <DialogFooter>
          <Button type="button" variant="ghost" onClick={onClose}>
            Cancel
          </Button>
          <Button type="button" onClick={() => void onSave()} disabled={pending}>
            {pending ? "Creating…" : "Create"}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}

// ---------- Delete pair ----------

function DeletePairDialog({
  storeName,
  pair,
  displayNameById,
  onClose,
  onDeleted,
}: {
  storeName: string;
  pair: WrapperResourcePairRecord;
  displayNameById: Map<string, string>;
  onClose: () => void;
  onDeleted: () => void;
}) {
  const [submitError, setSubmitError] = useState<string | null>(null);
  const mut = useMutation({
    mutationFn: () =>
      postPairDelete({
        store: storeName,
        wrapper: pair.wrapper,
        resource: pair.resource,
      }),
  });

  async function onDelete() {
    setSubmitError(null);
    try {
      const result = await mut.mutateAsync();
      if (result.ok) {
        onDeleted();
      }
      // result.ok === false means the backend refused; render the
      // blockers inline (handled below from `mut.data`).
    } catch (e) {
      setSubmitError(describeApiError(e));
    }
  }

  const blockers = mut.data?.blocked_by;

  return (
    <Dialog open onOpenChange={(o) => !o && onClose()}>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>
            Delete pair: {pair.wrapper} / {pair.resource}
          </DialogTitle>
          <DialogDescription>
            Removes the wrapper/resource pair record. Any wrapper
            permissions that reference this pair must be removed first.
          </DialogDescription>
        </DialogHeader>
        {blockers ? (
          <div
            role="alert"
            className="rounded-md border border-amber-500/30 bg-amber-500/10 px-3 py-2 text-xs text-amber-200"
          >
            Cannot delete — {blockers.permission_count} wrapper permission
            {blockers.permission_count === 1 ? "" : "s"} still reference this pair:
            <ul className="mt-1 list-disc pl-5">
              {blockers.sample_permissions.map((p) => (
                <li
                  key={`${p.client_instance}|${p.wrapper}|${p.resource}`}
                  className="font-mono"
                >
                  {displayNameById.get(p.client_instance) ?? p.client_instance}
                </li>
              ))}
            </ul>
            <p className="mt-2 text-amber-200/80">
              Clear those permissions in the table above, then try again.
            </p>
          </div>
        ) : null}
        {submitError ? (
          <p
            role="alert"
            className="rounded-md border border-rose-500/30 bg-rose-500/10 px-3 py-2 text-xs text-rose-300"
          >
            {submitError}
          </p>
        ) : null}
        <DialogFooter>
          <Button type="button" variant="ghost" onClick={onClose}>
            Cancel
          </Button>
          <Button
            type="button"
            variant="destructive"
            onClick={() => void onDelete()}
            disabled={mut.isPending}
            data-testid="wrappers-delete-pair-confirm"
          >
            {mut.isPending ? "Deleting…" : "Delete"}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}

// ---------- Cell editor ----------

function EditCellDialog({
  storeName,
  clientInstance,
  clientInstanceDisplayName,
  wrapper,
  resource,
  existing,
  canEdit,
  onClose,
}: {
  storeName: string;
  clientInstance: string;
  clientInstanceDisplayName: string;
  wrapper: string;
  resource: string;
  existing: WrapperPermission | null;
  canEdit: boolean;
  onClose: () => void;
}) {
  const qc = useQueryClient();
  const [kind, setKind] = useState<ModeKind>(
    existing ? (existing.mode.kind as ModeKind) : "locked",
  );
  const [untilLocal, setUntilLocal] = useState<string>(() => {
    if (existing && existing.mode.kind === "unlocked-until") {
      const epoch = Number(
        (existing.mode as { until_epoch: bigint | number }).until_epoch,
      );
      return toDatetimeLocal(new Date(epoch * 1000));
    }
    return toDatetimeLocal(new Date(Date.now() + 30 * 60 * 1000));
  });
  const [approvalKey, setApprovalKey] = useState<string>(() => {
    if (existing && existing.mode.kind === "per-call-approval") {
      return (existing.mode as { approval_key: string }).approval_key;
    }
    return "";
  });
  const [writable, setWritable] = useState<boolean>(existing?.writable ?? false);
  const [reason, setReason] = useState<string>(existing?.reason ?? "");
  const [submitError, setSubmitError] = useState<string | null>(null);

  const upsert = useMutation({
    mutationFn: (mode: WrapperApprovalMode) =>
      postWrapperPermissionUpsert({
        store: storeName,
        permission: {
          client_instance: clientInstance,
          wrapper,
          resource,
          mode,
          writable,
          reason: reason.trim() === "" ? undefined : reason.trim(),
        },
      }),
    onSuccess: async () => {
      await qc.invalidateQueries({ queryKey: ["admin", "wrapper-permissions", storeName] });
      onClose();
    },
    onError: (err) => setSubmitError(describeApiError(err)),
  });
  const remove = useMutation({
    mutationFn: () =>
      postWrapperPermissionDelete({
        store: storeName,
        client_instance: clientInstance,
        wrapper,
        resource,
      }),
    onSuccess: async () => {
      await qc.invalidateQueries({ queryKey: ["admin", "wrapper-permissions", storeName] });
      onClose();
    },
    onError: (err) => setSubmitError(describeApiError(err)),
  });

  function onSave() {
    setSubmitError(null);
    let mode: WrapperApprovalMode;
    switch (kind) {
      case "locked":
        mode = { kind: "locked" } as WrapperApprovalMode;
        break;
      case "unlocked":
        mode = { kind: "unlocked" } as WrapperApprovalMode;
        break;
      case "unlocked-until": {
        const until = new Date(untilLocal).getTime();
        if (Number.isNaN(until)) {
          setSubmitError("Pick a valid deadline.");
          return;
        }
        mode = {
          kind: "unlocked-until",
          until_epoch: Math.floor(until / 1000),
        } as unknown as WrapperApprovalMode;
        break;
      }
      case "per-call-approval": {
        if (approvalKey.trim() === "") {
          setSubmitError("Paste an OpenSSH public key.");
          return;
        }
        mode = {
          kind: "per-call-approval",
          approval_key: approvalKey.trim(),
        } as WrapperApprovalMode;
        break;
      }
    }
    upsert.mutate(mode);
  }

  return (
    <Dialog open onOpenChange={(o) => !o && onClose()}>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>
            {wrapper} / {resource} → {clientInstanceDisplayName}
          </DialogTitle>
          <DialogDescription>
            Set the mode{" "}
            <span title={clientInstance}>{clientInstanceDisplayName}</span>{" "}
            uses when calling{" "}
            <code className="rounded bg-(--color-surface-1) px-1 py-0.5">{wrapper}</code>{" "}
            for resource{" "}
            <code className="rounded bg-(--color-surface-1) px-1 py-0.5">{resource}</code>
            . The mode is checked on every read; per-call-approval also requires a
            fresh ssh-agent signature.
          </DialogDescription>
        </DialogHeader>
        {!canEdit ? (
          <p
            role="alert"
            className="rounded-md border border-amber-500/30 bg-amber-500/10 px-3 py-2 text-xs text-amber-200"
          >
            Only admin operators can change permissions.
          </p>
        ) : null}
        <div className="space-y-4">
          <fieldset
            disabled={!canEdit}
            className="flex flex-wrap gap-2"
            data-testid="mode-radio-group"
          >
            {(["locked", "unlocked", "unlocked-until", "per-call-approval"] as const).map(
              (k) => (
                <label
                  key={k}
                  className={
                    "inline-flex cursor-pointer items-center gap-2 rounded-md border px-3 py-1.5 text-sm " +
                    (kind === k
                      ? "border-emerald-500/40 bg-emerald-500/10 text-emerald-200"
                      : "border-(--color-line) bg-(--color-surface-1) text-(--color-fg)")
                  }
                >
                  <input
                    type="radio"
                    name="mode"
                    value={k}
                    data-testid={`mode-radio-${k}`}
                    className="sr-only"
                    checked={kind === k}
                    onChange={() => setKind(k)}
                  />
                  {k}
                </label>
              ),
            )}
          </fieldset>
          {kind === "unlocked-until" ? (
            <div className="space-y-1.5">
              <Label htmlFor="until-local">Deadline (local time)</Label>
              <Input
                id="until-local"
                type="datetime-local"
                value={untilLocal}
                onChange={(e) => setUntilLocal(e.target.value)}
                disabled={!canEdit}
              />
            </div>
          ) : null}
          {kind === "per-call-approval" ? (
            <div className="space-y-1.5">
              <Label htmlFor="approval-key">SSH public key (OpenSSH format)</Label>
              <textarea
                id="approval-key"
                rows={3}
                value={approvalKey}
                onChange={(e) => setApprovalKey(e.target.value)}
                disabled={!canEdit}
                spellCheck={false}
                className="block w-full rounded-md border border-(--color-line) bg-(--color-surface-0) px-3 py-2 font-mono text-xs text-(--color-fg)"
                placeholder="ssh-ed25519 AAAA..."
              />
            </div>
          ) : null}
          <label className="inline-flex cursor-pointer items-center gap-2 text-sm text-(--color-fg)">
            <input
              type="checkbox"
              data-testid="permission-writable"
              checked={writable}
              disabled={!canEdit}
              onChange={(e) => setWritable(e.target.checked)}
            />
            Allow wrapper-side writes for this resource (files / libsecret slots)
          </label>
          <div className="space-y-1.5">
            <Label htmlFor="reason">Reason (optional)</Label>
            <Input
              id="reason"
              value={reason}
              onChange={(e) => setReason(e.target.value)}
              disabled={!canEdit}
            />
          </div>
          {submitError ? (
            <p
              role="alert"
              className="rounded-md border border-rose-500/30 bg-rose-500/10 px-3 py-2 text-xs text-rose-300"
            >
              {submitError}
            </p>
          ) : null}
        </div>
        <DialogFooter className="sm:justify-between">
          {existing && canEdit ? (
            <Button
              type="button"
              variant="destructive"
              size="sm"
              onClick={() => remove.mutate()}
              disabled={remove.isPending}
              data-testid="permission-remove"
            >
              {remove.isPending ? "Removing…" : "Remove permission"}
            </Button>
          ) : (
            <span />
          )}
          <div className="flex gap-2">
            <Button type="button" variant="ghost" onClick={onClose}>
              Cancel
            </Button>
            <Button
              type="button"
              onClick={onSave}
              disabled={!canEdit || upsert.isPending}
              data-testid="permission-save"
            >
              {upsert.isPending ? "Saving…" : "Save"}
            </Button>
          </div>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}

function toDatetimeLocal(date: Date): string {
  // <input type="datetime-local"> wants `YYYY-MM-DDTHH:MM` in the
  // browser's local timezone with no timezone suffix. The
  // tzoffset arithmetic shifts a UTC Date back into local space
  // before slicing the ISO string.
  const offsetMs = date.getTimezoneOffset() * 60_000;
  const local = new Date(date.getTime() - offsetMs);
  return local.toISOString().slice(0, 16);
}

import { useEffect, useMemo, useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import {
  ShieldCheck,
  ShieldOff,
  Hourglass,
  KeyRound,
  Plus,
  FileText,
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
  type AdminAgentGrantsListResponse,
  type AdminCatalogGetResponse,
  type AdminClientInstancesListResponse,
  type AdminStoresListResponse,
  type AgentGrant,
  fetchClientInstances,
  fetchStores,
  postCatalogGet,
  postGrantDelete,
  postGrantUpsert,
  postGrantsList,
  type WhoamiResponse,
  type WrapperApprovalMode,
} from "@/api/client";
import { describeApiError } from "@/lib/error-message";

interface MatrixPageProps {
  principal: WhoamiResponse | null;
}

/** Canonical kebab string for a grant mode. */
export type ModeKind =
  | "locked"
  | "unlocked"
  | "unlocked-until"
  | "per-call-approval";

/** `WrapperApprovalMode` is a tagged union over `kind`; this maps to it. */
export function modeKind(mode: WrapperApprovalMode): ModeKind {
  return mode.kind as ModeKind;
}

/** A `(wrapper, resource)` pair — one row in the matrix. */
export interface WrapperResource {
  wrapper: string;
  resource: string;
}

export function wrapperResourceKey(wr: WrapperResource): string {
  return `${wr.wrapper}|${wr.resource}`;
}

/** Collect every distinct runtime client instance mentioned in the catalog
 *  + grants list, sorted for deterministic table rendering. */
export function deriveClientInstances(
  catalog: AdminCatalogGetResponse | undefined,
  grants: AgentGrant[],
): string[] {
  const set = new Set<string>();
  for (const e of catalog?.entries ?? []) {
    if (e.client_instance) set.add(e.client_instance);
  }
  for (const g of grants) set.add(g.client_instance);
  return [...set].sort();
}

/** Collect every distinct `(wrapper, resource)` pair mentioned in the
 *  catalog + grants list, sorted. */
export function deriveWrapperResources(
  catalog: AdminCatalogGetResponse | undefined,
  grants: AgentGrant[],
): WrapperResource[] {
  const map = new Map<string, WrapperResource>();
  for (const e of catalog?.entries ?? []) {
    const wr = { wrapper: e.wrapper, resource: e.resource };
    map.set(wrapperResourceKey(wr), wr);
  }
  for (const g of grants) {
    const wr = { wrapper: g.wrapper, resource: g.resource };
    map.set(wrapperResourceKey(wr), wr);
  }
  return [...map.values()].sort((a, b) =>
    a.wrapper === b.wrapper
      ? a.resource.localeCompare(b.resource)
      : a.wrapper.localeCompare(b.wrapper),
  );
}

export function MatrixPage({ principal }: MatrixPageProps) {
  useEffect(() => {
    document.title = "underwrap — matrix";
  }, []);

  const isAdmin = !!(principal?.authenticated && principal.role === "admin");

  // Stores → pick first granted by default. Admin sees everything.
  const stores = useQuery<AdminStoresListResponse>({
    queryKey: ["admin", "stores"],
    queryFn: fetchStores,
  });
  // URL-persisted store selector: `?store=name`.
  const initialStore = (() => {
    if (typeof window === "undefined") return null;
    return new URLSearchParams(window.location.search).get("store");
  })();
  const [storeName, setStoreName] = useState<string | null>(initialStore);

  // When the stores list lands, default the selector to its first
  // entry if we don't already have a URL hint.
  useEffect(() => {
    if (storeName) return;
    const first = stores.data?.items[0]?.id;
    if (first) setStoreName(first);
  }, [storeName, stores.data]);

  // Keep the URL in sync so a refresh preserves the selection.
  useEffect(() => {
    if (!storeName) return;
    const url = new URL(window.location.href);
    url.searchParams.set("store", storeName);
    window.history.replaceState({}, "", url.toString());
  }, [storeName]);

  const grants = useQuery<AdminAgentGrantsListResponse>({
    queryKey: ["admin", "grants", storeName],
    queryFn: () => postGrantsList({ store: storeName! }),
    enabled: !!storeName,
  });
  const catalog = useQuery<AdminCatalogGetResponse>({
    queryKey: ["admin", "catalog", storeName],
    queryFn: () => postCatalogGet({ store: storeName! }),
    enabled: !!storeName,
  });
  // The catalog and grants carry the runtime client *id*; the
  // operator-facing column header shows the editable display name
  // resolved from this list.
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
  const wrapperResources = useMemo(
    () => deriveWrapperResources(catalog.data, grants.data?.grants ?? []),
    [catalog.data, grants.data],
  );
  const clientInstances = useMemo(
    () => deriveClientInstances(catalog.data, grants.data?.grants ?? []),
    [catalog.data, grants.data],
  );

  /** Map `(client_instance, wrapper, resource)` → grant. */
  const grantIndex = useMemo(() => {
    const map = new Map<string, AgentGrant>();
    for (const g of grants.data?.grants ?? []) {
      map.set(`${g.client_instance}|${g.wrapper}|${g.resource}`, g);
    }
    return map;
  }, [grants.data]);

  const [editing, setEditing] = useState<{
    clientInstance: string;
    wrapper: string;
    resource: string;
    grant: AgentGrant | null;
  } | null>(null);
  const [rawOpen, setRawOpen] = useState(false);

  if (!storeName) {
    return (
      <div className="space-y-4">
        <h2 className="text-xl font-semibold text-(--color-fg)">Matrix</h2>
        {stores.isLoading ? (
          <p className="text-sm text-(--color-muted)">Loading…</p>
        ) : (
          <p
            role="alert"
            className="rounded-md border border-amber-500/30 bg-amber-500/10 px-3 py-2 text-sm text-amber-200"
          >
            No stores are visible to you. Ask an admin for a grant on
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
          <h2 className="text-xl font-semibold text-(--color-fg)">Matrix</h2>
          <p className="mt-1 text-sm text-(--color-muted)">
            Agent grants for each{" "}
            <code className="rounded bg-(--color-surface-1) px-1 py-0.5 text-xs">
              (client-instance, wrapper, resource)
            </code>{" "}
            tuple. Click a cell to set or change the mode.
          </p>
        </div>
        <div className="flex items-center gap-3">
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
              data-testid="matrix-store-select"
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

      {grants.isLoading || catalog.isLoading ? (
        <p className="text-sm text-(--color-muted)">Loading matrix…</p>
      ) : grants.error || catalog.error ? (
        <p role="alert" className="text-sm text-rose-300">
          {describeApiError(grants.error ?? catalog.error)}
        </p>
      ) : wrapperResources.length === 0 || clientInstances.length === 0 ? (
        <p className="text-sm text-(--color-muted)">
          The matrix is empty for this store. Once an agent registers with the
          backend, its row appears here.
        </p>
      ) : (
        <MatrixTable
          wrapperResources={wrapperResources}
          clientInstances={clientInstances}
          displayNameById={displayNameById}
          grantIndex={grantIndex}
          isAdmin={isAdmin}
          onCellClick={(wr, clientInstance, grant) =>
            setEditing({
              clientInstance,
              wrapper: wr.wrapper,
              resource: wr.resource,
              grant,
            })
          }
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
          existing={editing.grant}
          canEdit={isAdmin}
          onClose={() => setEditing(null)}
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

function MatrixTable({
  wrapperResources,
  clientInstances,
  displayNameById,
  grantIndex,
  isAdmin,
  onCellClick,
}: {
  wrapperResources: WrapperResource[];
  clientInstances: string[];
  displayNameById: Map<string, string>;
  grantIndex: Map<string, AgentGrant>;
  isAdmin: boolean;
  onCellClick: (
    wr: WrapperResource,
    clientInstance: string,
    grant: AgentGrant | null,
  ) => void;
}) {
  return (
    <div className="overflow-x-auto rounded-xl border border-(--color-line)">
      <table className="min-w-full text-left text-sm">
        <thead className="bg-(--color-surface-0)/60 text-xs uppercase tracking-wide text-(--color-muted)">
          <tr>
            <th className="sticky left-0 z-10 bg-(--color-surface-0)/60 px-4 py-2">
              Wrapper
            </th>
            <th className="sticky left-0 z-10 bg-(--color-surface-0)/60 px-4 py-2">
              Resource
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
          </tr>
        </thead>
        <tbody>
          {wrapperResources.map((wr) => (
            <tr
              key={wrapperResourceKey(wr)}
              className="border-t border-(--color-line) align-middle"
            >
              <th
                scope="row"
                className="sticky left-0 z-10 bg-(--color-surface-0)/40 px-4 py-3 text-left font-medium text-(--color-fg)"
              >
                {wr.wrapper}
              </th>
              <td className="px-4 py-3 text-(--color-muted)">{wr.resource}</td>
              {clientInstances.map((c) => {
                const key = `${c}|${wr.wrapper}|${wr.resource}`;
                const grant = grantIndex.get(key) ?? null;
                return (
                  <td key={key} className="px-3 py-3">
                    <button
                      type="button"
                      data-testid={`matrix-cell-${wr.wrapper}-${wr.resource}-${c}`}
                      onClick={() => onCellClick(wr, c, grant)}
                      disabled={!isAdmin}
                      className="min-h-11 min-w-[5rem] rounded-md border border-(--color-line) bg-(--color-surface-1) px-2 py-1 text-xs hover:bg-(--color-surface-0) disabled:opacity-60"
                    >
                      <CellContent grant={grant} />
                    </button>
                  </td>
                );
              })}
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}

function CellContent({ grant }: { grant: AgentGrant | null }) {
  if (!grant) {
    return (
      <span className="inline-flex items-center gap-1 text-(--color-muted)">
        <Plus className="h-3 w-3" aria-hidden /> not granted
      </span>
    );
  }
  return <ModeBadge mode={grant.mode} />;
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
  existing: AgentGrant | null;
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
    // Default deadline: now + 30 minutes, in the visitor's local
    // timezone for the <input type="datetime-local">.
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
      postGrantUpsert({
        store: storeName,
        grant: {
          client_instance: clientInstance,
          wrapper,
          resource,
          mode,
          writable,
          reason: reason.trim() === "" ? undefined : reason.trim(),
        },
      }),
    onSuccess: async () => {
      await qc.invalidateQueries({ queryKey: ["admin", "grants", storeName] });
      onClose();
    },
    onError: (err) => setSubmitError(describeApiError(err)),
  });
  const remove = useMutation({
    mutationFn: () =>
      postGrantDelete({
        store: storeName,
        client_instance: clientInstance,
        wrapper,
        resource,
      }),
    onSuccess: async () => {
      await qc.invalidateQueries({ queryKey: ["admin", "grants", storeName] });
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
        // ts-rs types `until_epoch` as `bigint` (u64 in Rust), but
        // JSON has no BigInt — over the wire it must be a plain
        // number, which serde happily deserialises into u64.
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
            Only admin operators can change grants.
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
              data-testid="grant-writable"
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
              data-testid="grant-remove"
            >
              {remove.isPending ? "Removing…" : "Remove grant"}
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
              data-testid="grant-save"
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

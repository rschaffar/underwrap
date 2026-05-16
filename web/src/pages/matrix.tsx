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
  type AdminStoresListResponse,
  type AgentGrant,
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

export interface Principal {
  host: string;
  user: string;
}

/** Stable display key for a `(host, user)` pair. */
export function principalKey(p: Principal): string {
  return `${p.host}/${p.user}`;
}

/** Collect every distinct `(host, user)` mentioned in the catalog + grants
 *  list, sorted for deterministic table rendering. */
export function derivePrincipals(
  catalog: AdminCatalogGetResponse | undefined,
  grants: AgentGrant[],
): Principal[] {
  const set = new Map<string, Principal>();
  for (const e of catalog?.entries ?? []) {
    // Catalog uses `*` as the wildcard for "all users"; surface it
    // verbatim so operators recognise it.
    set.set(principalKey({ host: e.host, user: e.user }), {
      host: e.host,
      user: e.user,
    });
  }
  for (const g of grants) {
    set.set(principalKey({ host: g.host, user: g.user }), {
      host: g.host,
      user: g.user,
    });
  }
  return [...set.values()].sort((a, b) =>
    a.host === b.host ? a.user.localeCompare(b.user) : a.host.localeCompare(b.host),
  );
}

/** Collect every distinct wrapper name mentioned in the catalog + grants
 *  list, sorted. */
export function deriveWrappers(
  catalog: AdminCatalogGetResponse | undefined,
  grants: AgentGrant[],
): string[] {
  const set = new Set<string>();
  for (const e of catalog?.entries ?? []) set.add(e.wrapper);
  for (const g of grants) set.add(g.wrapper);
  return [...set].sort();
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
    const first = stores.data?.items[0]?.name;
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
  const wrappers = useMemo(
    () => deriveWrappers(catalog.data, grants.data?.grants ?? []),
    [catalog.data, grants.data],
  );
  const principals = useMemo(
    () => derivePrincipals(catalog.data, grants.data?.grants ?? []),
    [catalog.data, grants.data],
  );

  /** Map `(wrapper, host, user)` → grant. Cheap; rebuilt on every render. */
  const grantIndex = useMemo(() => {
    const map = new Map<string, AgentGrant>();
    for (const g of grants.data?.grants ?? []) {
      map.set(`${g.wrapper}|${g.host}|${g.user}`, g);
    }
    return map;
  }, [grants.data]);

  const [editing, setEditing] = useState<{
    wrapper: string;
    host: string;
    user: string;
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
            Agent grants for each <code className="rounded bg-(--color-surface-1) px-1 py-0.5 text-xs">(wrapper, host, user)</code> tuple.
            Click a cell to set or change the mode.
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
                <option key={s.name} value={s.name}>
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
      ) : wrappers.length === 0 || principals.length === 0 ? (
        <p className="text-sm text-(--color-muted)">
          The matrix is empty for this store. Once an agent registers with the
          backend, its row appears here.
        </p>
      ) : (
        <MatrixTable
          wrappers={wrappers}
          principals={principals}
          grantIndex={grantIndex}
          isAdmin={isAdmin}
          onCellClick={(wrapper, p, grant) =>
            setEditing({ wrapper, host: p.host, user: p.user, grant })
          }
        />
      )}

      {editing ? (
        <EditCellDialog
          storeName={storeName}
          wrapper={editing.wrapper}
          host={editing.host}
          user={editing.user}
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
  wrappers,
  principals,
  grantIndex,
  isAdmin,
  onCellClick,
}: {
  wrappers: string[];
  principals: Principal[];
  grantIndex: Map<string, AgentGrant>;
  isAdmin: boolean;
  onCellClick: (wrapper: string, principal: Principal, grant: AgentGrant | null) => void;
}) {
  return (
    <div className="overflow-x-auto rounded-xl border border-(--color-line)">
      <table className="min-w-full text-left text-sm">
        <thead className="bg-(--color-surface-0)/60 text-xs uppercase tracking-wide text-(--color-muted)">
          <tr>
            <th className="sticky left-0 z-10 bg-(--color-surface-0)/60 px-4 py-2">
              Wrapper
            </th>
            {principals.map((p) => (
              <th key={principalKey(p)} className="px-3 py-2">
                <div className="font-medium text-(--color-fg)">{p.host}</div>
                <div className="text-(--color-muted)">{p.user}</div>
              </th>
            ))}
          </tr>
        </thead>
        <tbody>
          {wrappers.map((w) => (
            <tr key={w} className="border-t border-(--color-line) align-middle">
              <th
                scope="row"
                className="sticky left-0 z-10 bg-(--color-surface-0)/40 px-4 py-3 text-left font-medium text-(--color-fg)"
              >
                {w}
              </th>
              {principals.map((p) => {
                const key = `${w}|${p.host}|${p.user}`;
                const grant = grantIndex.get(key) ?? null;
                return (
                  <td key={key} className="px-3 py-3">
                    <button
                      type="button"
                      data-testid={`matrix-cell-${w}-${p.host}-${p.user}`}
                      onClick={() => onCellClick(w, p, grant)}
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
  wrapper,
  host,
  user,
  existing,
  canEdit,
  onClose,
}: {
  storeName: string;
  wrapper: string;
  host: string;
  user: string;
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
  const [reason, setReason] = useState<string>(existing?.reason ?? "");
  const [submitError, setSubmitError] = useState<string | null>(null);

  const upsert = useMutation({
    mutationFn: (mode: WrapperApprovalMode) =>
      postGrantUpsert({
        store: storeName,
        grant: {
          wrapper,
          host,
          user,
          mode,
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
    mutationFn: () => postGrantDelete({ store: storeName, wrapper, host, user }),
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
            {wrapper} → {host} / {user}
          </DialogTitle>
          <DialogDescription>
            Set the mode this agent uses when calling{" "}
            <code className="rounded bg-(--color-surface-1) px-1 py-0.5">{wrapper}</code>
            . The mode is checked on every read; per-call-approval also
            requires a fresh ssh-agent signature.
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

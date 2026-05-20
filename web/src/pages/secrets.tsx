import { useEffect, useMemo, useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useForm } from "react-hook-form";
import { zodResolver } from "@hookform/resolvers/zod";
import { z } from "zod";
import { ChevronRight, Eye, Pencil, Plus, Trash2 } from "lucide-react";

import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import {
  Form,
  FormControl,
  FormDescription,
  FormField,
  FormItem,
  FormLabel,
  FormMessage,
} from "@/components/ui/form";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import {
  type AdminClientInstancesListResponse,
  type AdminResourcesListResponse,
  type AdminSecretEntry,
  type AdminSecretListResponse,
  type AdminSecretReadResponse,
  type AdminStoresListResponse,
  type EntityCreator,
  fetchClientInstances,
  fetchStores,
  postResourceCreate,
  postResourceDelete,
  postResourceRename,
  postResourcesList,
  postSecretDelete,
  postSecretList,
  postSecretRead,
  postSecretRename,
  postSecretWrite,
  type ResourceRecord,
  type ResourceSlot,
  type WhoamiResponse,
} from "@/api/client";
import { describeApiError } from "@/lib/error-message";

interface SecretsPageProps {
  /**
   * Whoami payload from the router context. Used to gate
   * destructive actions for non-admin operators (though the backend
   * is the source of truth).
   */
  principal: WhoamiResponse | null;
}

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
function resourceCount(g: SecretResourceGroup): number {
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
    g.env.sort((a, b) => a.key.localeCompare(b.key));
    g.files.sort((a, b) => a.key.localeCompare(b.key));
    g.libsecret.sort((a, b) => a.key.localeCompare(b.key));
  }
  return [...groups.values()].sort((a, b) =>
    a.resource_id.localeCompare(b.resource_id),
  );
}

export function SecretsPage({ principal: _principal }: SecretsPageProps) {
  useEffect(() => {
    document.title = "underwrap — secrets";
  }, []);

  // Store selector — URL-persisted, same shape as the Wrapper permissions page.
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

  const list = useQuery<AdminSecretListResponse>({
    queryKey: ["admin", "secrets", storeName],
    queryFn: () => postSecretList({ store: storeName! }),
    enabled: !!storeName,
  });
  // Resources are first-class backend records — these drive the
  // "empty resource" rows (resources that exist but have no secrets
  // yet) and the provenance display.
  const resourcesList = useQuery<AdminResourcesListResponse>({
    queryKey: ["admin", "resources", storeName],
    queryFn: () => postResourcesList({ store: storeName! }),
    enabled: !!storeName,
  });
  // Client-instance directory powers `displayNameById` so resource
  // provenance shows the operator-facing name instead of the opaque
  // backend id when a client instance auto-created a resource.
  const clientInstancesList = useQuery<AdminClientInstancesListResponse>({
    queryKey: ["admin", "client-instances"],
    queryFn: fetchClientInstances,
  });
  const displayNameById = useMemo(() => {
    const map = new Map<string, string>();
    for (const i of clientInstancesList.data?.items ?? []) {
      map.set(i.id, i.display_name);
    }
    return map;
  }, [clientInstancesList.data]);
  const isAdmin = !!(_principal?.authenticated && _principal.role === "admin");
  const resourceProvenance = useMemo(() => {
    const map = new Map<string, ResourceRecord>();
    for (const r of resourcesList.data?.resources ?? []) {
      map.set(r.id, r);
    }
    return map;
  }, [resourcesList.data]);

  const grouped = useMemo(
    () => groupSecretEntries(list.data?.entries ?? []),
    [list.data],
  );

  // Resources known to this store, keyed internally by resource id.
  // The catalog provides the display name; orphan groups (a resource
  // id that appears in secrets but not in the catalog) fall back to
  // displaying the id.
  const resourceNameById = useMemo(() => {
    const map = new Map<string, string>();
    for (const r of resourcesList.data?.resources ?? []) {
      map.set(r.id, r.name);
    }
    return map;
  }, [resourcesList.data]);
  const allResources = useMemo<SecretResourceGroup[]>(() => {
    const byResource = new Map<string, SecretResourceGroup>();
    for (const g of grouped) {
      byResource.set(g.resource_id, g);
    }
    for (const record of resourcesList.data?.resources ?? []) {
      if (!byResource.has(record.id)) {
        byResource.set(record.id, {
          resource_id: record.id,
          env: [],
          files: [],
          libsecret: [],
        });
      }
    }
    return [...byResource.values()].sort((a, b) => {
      const an = resourceNameById.get(a.resource_id) ?? a.resource_id;
      const bn = resourceNameById.get(b.resource_id) ?? b.resource_id;
      return an.localeCompare(bn);
    });
  }, [grouped, resourcesList.data, resourceNameById]);

  const [revealing, setRevealing] = useState<SecretAddress | null>(null);
  const [editing, setEditing] = useState<SecretAddress | null>(null);
  const [creating, setCreating] = useState<
    { resource_id: string; slot: SecretSlot } | null
  >(null);
  const [creatingResource, setCreatingResource] = useState(false);
  const [deleting, setDeleting] = useState<SecretAddress | null>(null);
  const [deletingResource, setDeletingResource] = useState<string | null>(null);
  const [renamingResource, setRenamingResource] = useState<string | null>(null);

  if (!storeName) {
    return (
      <div className="space-y-4">
        <h2 className="text-xl font-semibold text-(--color-fg)">Secrets</h2>
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
          <h2 className="text-xl font-semibold text-(--color-fg)">Secrets</h2>
          <p className="mt-1 text-sm text-(--color-muted)">
            Stored credential material. Per-resource toggles in each
            block decide whether the wrapper can write back to
            <code>files</code> or <code>libsecret</code> entries;
            admin writes through this page are always allowed.
          </p>
        </div>
        <div className="flex items-center gap-3">
          <Button
            type="button"
            size="sm"
            onClick={() => setCreatingResource(true)}
          >
            <Plus className="mr-1.5 h-4 w-4" aria-hidden />
            Create resource
          </Button>
          <label className="text-sm text-(--color-muted)">
            Store:{" "}
            <select
              data-testid="secrets-store-select"
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

      {list.isLoading ? (
        <p className="text-sm text-(--color-muted)">Loading…</p>
      ) : list.error ? (
        <p role="alert" className="text-sm text-rose-300">
          {describeApiError(list.error)}
        </p>
      ) : allResources.length === 0 ? (
        <p className="text-sm text-(--color-muted)">
          No resources are known to this store yet. Use{" "}
          <strong className="text-(--color-fg)">Create resource</strong>{" "}
          to add one, or wait for a wrapper to auto-create one on
          startup (when its client instance is opted in).
        </p>
      ) : (
        <div className="space-y-2">
          {allResources.map((group) => (
            <ResourceBlock
              key={group.resource_id}
              group={group}
              provenance={resourceProvenance.get(group.resource_id)}
              displayNameById={displayNameById}
              isAdmin={isAdmin}
              onReveal={(entry) =>
                setRevealing({
                  resource_id: group.resource_id,
                  slot: entry.slot,
                  key: entry.key,
                })
              }
              onEdit={(entry) =>
                setEditing({
                  resource_id: group.resource_id,
                  slot: entry.slot,
                  key: entry.key,
                })
              }
              onDelete={(entry) =>
                setDeleting({
                  resource_id: group.resource_id,
                  slot: entry.slot,
                  key: entry.key,
                })
              }
              onDeleteResource={() => setDeletingResource(group.resource_id)}
              onRenameResource={() => setRenamingResource(group.resource_id)}
              onAddEntry={(slot) =>
                setCreating({ resource_id: group.resource_id, slot })
              }
            />
          ))}
        </div>
      )}

      {revealing ? (
        <RevealSecretDialog
          storeName={storeName}
          address={revealing}
          onClose={() => setRevealing(null)}
        />
      ) : null}
      {creating ? (
        <WriteSecretDialog
          storeName={storeName}
          mode="create"
          address={{ ...creating, key: "" }}
          onClose={() => setCreating(null)}
        />
      ) : null}
      {creatingResource ? (
        <CreateResourceDialog
          storeName={storeName}
          existingResources={(resourcesList.data?.resources ?? []).map(
            (r) => r.name,
          )}
          onClose={() => setCreatingResource(false)}
        />
      ) : null}
      {editing ? (
        <WriteSecretDialog
          storeName={storeName}
          mode="edit"
          address={editing}
          onClose={() => setEditing(null)}
        />
      ) : null}
      {deleting ? (
        <DeleteSecretDialog
          storeName={storeName}
          address={deleting}
          onClose={() => setDeleting(null)}
        />
      ) : null}
      {deletingResource ? (
        <DeleteResourceDialog
          storeName={storeName}
          resourceId={deletingResource}
          resourceName={resourceNameById.get(deletingResource) ?? deletingResource}
          onClose={() => setDeletingResource(null)}
        />
      ) : null}
      {renamingResource ? (
        <RenameResourceDialog
          storeName={storeName}
          resourceId={renamingResource}
          currentName={
            resourceNameById.get(renamingResource) ?? renamingResource
          }
          existingNames={(resourcesList.data?.resources ?? []).map((r) => r.name)}
          onClose={() => setRenamingResource(null)}
        />
      ) : null}
    </div>
  );
}

// ---------- Resource block (expandable) ----------

interface RowActions {
  onReveal: (entry: SecretEntry) => void;
  onEdit: (entry: SecretEntry) => void;
  onDelete: (entry: SecretEntry) => void;
}

function ResourceBlock({
  group,
  provenance,
  displayNameById,
  isAdmin,
  onAddEntry,
  onDeleteResource,
  onRenameResource,
  ...actions
}: {
  group: SecretResourceGroup;
  provenance: ResourceRecord | undefined;
  displayNameById: Map<string, string>;
  isAdmin: boolean;
  onAddEntry: (slot: SecretSlot) => void;
  onDeleteResource: () => void;
  onRenameResource: () => void;
} & RowActions) {
  const total = resourceCount(group);
  // The summary line shows which slots have content. It avoids
  // overpromising an empty slot section before the operator
  // expands.
  const summary = SECRET_SLOTS.filter((slot) => group[slot].length > 0)
    .map((slot) => `${slot} (${group[slot].length})`)
    .join(" · ");
  const provenanceLabel = provenance
    ? describeCreator(provenance.created_by, displayNameById)
    : null;
  const provenanceDate =
    provenance && Number(provenance.created_at)
      ? new Date(Number(provenance.created_at) * 1000)
      : null;
  // Display the operator-facing name when the catalog knows about
  // this resource; orphan groups fall back to the id.
  const displayLabel = provenance?.name ?? group.resource_id;
  return (
    <details
      data-testid={`resource-block-${group.resource_id}`}
      className="group rounded-md border border-(--color-line) bg-(--color-surface-0)/40 open:bg-(--color-surface-0)/70"
    >
      <summary className="flex cursor-pointer list-none items-center gap-2 px-4 py-3 text-sm">
        <ChevronRight
          className="h-4 w-4 transition-transform group-open:rotate-90"
          aria-hidden
        />
        <span
          className="font-mono text-(--color-fg)"
          title={`id: ${group.resource_id}`}
        >
          {displayLabel}
        </span>
        {provenanceLabel ? (
          <span
            className="text-xs text-(--color-muted)"
            title={
              provenanceDate
                ? `Created ${provenanceDate.toISOString()} by ${provenanceLabel}`
                : undefined
            }
            data-testid={`resource-provenance-${group.resource_id}`}
          >
            · by {provenanceLabel}
            {provenanceDate ? ` on ${provenanceDate.toLocaleDateString()}` : null}
          </span>
        ) : null}
        <span className="ml-auto text-xs text-(--color-muted)">
          {total === 0 ? "empty" : summary}
        </span>
        {isAdmin && provenance ? (
          <>
            <Button
              type="button"
              variant="ghost"
              size="sm"
              aria-label={`Rename resource ${displayLabel}`}
              data-testid={`resource-rename-${group.resource_id}`}
              onClick={(e) => {
                e.preventDefault();
                e.stopPropagation();
                onRenameResource();
              }}
              className="h-7 px-2"
            >
              <Pencil className="h-3.5 w-3.5" aria-hidden />
            </Button>
            <Button
              type="button"
              variant="ghost"
              size="sm"
              aria-label={`Delete resource ${displayLabel}`}
              data-testid={`resource-delete-${group.resource_id}`}
              onClick={(e) => {
                e.preventDefault();
                e.stopPropagation();
                onDeleteResource();
              }}
              className="h-7 px-2"
            >
              <Trash2 className="h-3.5 w-3.5" aria-hidden />
            </Button>
          </>
        ) : null}
      </summary>
      <div className="space-y-3 border-t border-(--color-line)/60 px-4 py-3">
        {SECRET_SLOTS.map((slot) => (
          <SlotSection
            key={slot}
            slot={slot}
            entries={group[slot]}
            isAdmin={isAdmin}
            onAdd={() => onAddEntry(slot)}
            {...actions}
          />
        ))}
      </div>
    </details>
  );
}

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

function SlotSection({
  slot,
  entries,
  isAdmin: _isAdmin,
  onAdd,
  ...actions
}: {
  slot: SecretSlot;
  entries: SecretEntry[];
  isAdmin: boolean;
  onAdd: () => void;
} & RowActions) {
  return (
    <section data-testid={`slot-${slot}`} className="space-y-1">
      <div className="flex items-center gap-2">
        <h3 className="font-mono text-xs uppercase tracking-wide text-(--color-muted)">
          {slot}
        </h3>
        <Button
          type="button"
          variant="ghost"
          size="sm"
          aria-label={`Add ${slot} entry`}
          data-testid={`slot-${slot}-add`}
          onClick={onAdd}
          className="h-6 px-1.5"
        >
          <Plus className="h-3.5 w-3.5" aria-hidden />
        </Button>
      </div>
      {entries.length === 0 ? (
        <p className="pl-2 text-xs text-(--color-muted)/70">(none)</p>
      ) : (
        <ul className="space-y-1">
          {entries.map((entry) => (
            <li
              key={`${entry.slot}|${entry.key}`}
              className="flex flex-col gap-1 rounded-md px-2 py-1.5 hover:bg-(--color-surface-1)/50 sm:flex-row sm:items-center sm:gap-2"
            >
              <span className="font-mono text-xs text-(--color-fg)">
                {entry.key}
              </span>
              <div className="ml-auto inline-flex gap-2">
                <EntryActions entry={entry} {...actions} />
              </div>
            </li>
          ))}
        </ul>
      )}
    </section>
  );
}

function EntryActions({
  entry,
  onReveal,
  onEdit,
  onDelete,
}: { entry: SecretEntry } & RowActions) {
  return (
    <>
      <Button variant="ghost" size="sm" onClick={() => onReveal(entry)}>
        <Eye className="mr-1.5 h-4 w-4" aria-hidden />
        Reveal
      </Button>
      <Button variant="ghost" size="sm" onClick={() => onEdit(entry)}>
        <Pencil className="mr-1.5 h-4 w-4" aria-hidden />
        Edit
      </Button>
      <Button variant="destructive" size="sm" onClick={() => onDelete(entry)}>
        <Trash2 className="mr-1.5 h-4 w-4" aria-hidden />
        Delete
      </Button>
    </>
  );
}

// ---------- Reveal ----------

function RevealSecretDialog({
  storeName,
  address,
  onClose,
}: {
  storeName: string;
  address: SecretAddress;
  onClose: () => void;
}) {
  const [confirmed, setConfirmed] = useState(false);
  const [copied, setCopied] = useState(false);
  const read = useQuery<AdminSecretReadResponse>({
    queryKey: [
      "admin",
      "secret",
      "read",
      storeName,
      address.resource_id,
      address.slot,
      address.key,
    ],
    queryFn: () =>
      postSecretRead({
        store: storeName,
        resource_id: address.resource_id,
        slot: address.slot,
        key: address.key,
      }),
    // Only fire after the operator confirms they want to view it.
    enabled: confirmed,
    // Don't cache the secret material in the React Query store any
    // longer than necessary.
    staleTime: 0,
    gcTime: 0,
    refetchOnMount: "always",
  });

  const decoded = useMemo(() => {
    if (!read.data?.material_b64) return null;
    try {
      // atob may throw on malformed input; this is data from our own
      // backend so success is the expected path.
      return atob(read.data.material_b64);
    } catch {
      return null;
    }
  }, [read.data]);

  async function onCopy() {
    if (!decoded) return;
    try {
      await navigator.clipboard.writeText(decoded);
      setCopied(true);
      window.setTimeout(() => setCopied(false), 1500);
    } catch {
      // Non-secure-context fallback: do nothing; the textarea is
      // selectable for manual copy.
    }
  }

  return (
    <Dialog open onOpenChange={(o) => !o && onClose()}>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>Reveal secret</DialogTitle>
          <DialogDescription>
            <code>
              {address.resource_id} · {address.slot} · {address.key}
            </code>{" "}
            from <code>{storeName}</code>.
          </DialogDescription>
        </DialogHeader>

        {!confirmed ? (
          <>
            <p className="text-xs text-(--color-muted)">
              Press <strong>Show</strong> to fetch the value. It will
              be displayed in plain text in this dialog.
            </p>
            <DialogFooter>
              <Button type="button" variant="ghost" onClick={onClose}>
                Cancel
              </Button>
              <Button type="button" onClick={() => setConfirmed(true)}>
                Show
              </Button>
            </DialogFooter>
          </>
        ) : read.isLoading ? (
          <p className="text-sm text-(--color-muted)">Loading…</p>
        ) : read.error ? (
          <p
            role="alert"
            className="rounded-md border border-rose-500/30 bg-rose-500/10 px-3 py-2 text-xs text-rose-300"
          >
            {describeApiError(read.error)}
          </p>
        ) : decoded == null ? (
          <p
            role="alert"
            className="rounded-md border border-amber-500/30 bg-amber-500/10 px-3 py-2 text-xs text-amber-200"
          >
            {read.data?.material_b64 == null
              ? "The secret does not exist (or was removed)."
              : "The secret material could not be decoded as UTF-8 text."}
          </p>
        ) : (
          <div className="space-y-2">
            <Label htmlFor="reveal-value">Value</Label>
            <textarea
              id="reveal-value"
              data-testid="reveal-value"
              readOnly
              value={decoded}
              rows={6}
              className="w-full rounded-md border border-(--color-line) bg-(--color-surface-0) p-2 font-mono text-xs text-(--color-fg)"
            />
            <Button type="button" variant="ghost" size="sm" onClick={onCopy}>
              {copied ? "Copied" : "Copy to clipboard"}
            </Button>
          </div>
        )}

        {confirmed ? (
          <DialogFooter>
            <Button type="button" onClick={onClose}>
              Close
            </Button>
          </DialogFooter>
        ) : null}
      </DialogContent>
    </Dialog>
  );
}

// ---------- Write (create + edit share the dialog) ----------
//
// The dialog edits the structured `(resource, slot, key)` address
// directly. Resource and slot are locked in both modes — they're
// pinned by the page row that opened the dialog, and changing them
// would amount to moving an entry across resources/slots, which the
// API doesn't model as a single operation. Key is editable in both
// modes; on edit, a key change is implemented as write-new +
// delete-old (best-effort: a failure between the two leaves both
// entries present until the operator deletes one manually).

const writeSchema = z.object({
  key: z.string().min(1, "Key is required."),
  material: z.string().min(1, "Material is required."),
});
type WriteValues = z.infer<typeof writeSchema>;

function WriteSecretDialog({
  storeName,
  mode,
  address,
  onClose,
}: {
  storeName: string;
  mode: "create" | "edit";
  /**
   * The address the dialog opens against. In `create` mode `key` is
   * the empty string; in `edit` mode it's the existing key.
   */
  address: SecretAddress;
  onClose: () => void;
}) {
  const qc = useQueryClient();
  const [submitError, setSubmitError] = useState<string | null>(null);
  const form = useForm<WriteValues>({
    resolver: zodResolver(writeSchema),
    defaultValues: { key: address.key, material: "" },
  });

  const writeMut = useMutation({
    mutationFn: (v: { key: string; material: string }) =>
      postSecretWrite({
        store: storeName,
        resource_id: address.resource_id,
        slot: address.slot,
        key: v.key,
        material_b64: btoa(v.material),
      }),
  });
  // The atomic /v1/admin/secret/rename does load-blob → swap-entry →
  // save in one handler, so a key change can never leave the operator
  // with two rows visible. Used only on the edit-with-key-change path.
  const renameMut = useMutation({
    mutationFn: (v: { from_key: string; to_key: string; material?: string }) =>
      postSecretRename({
        store: storeName,
        resource_id: address.resource_id,
        slot: address.slot,
        from_key: v.from_key,
        to_key: v.to_key,
        material_b64: v.material === undefined ? undefined : btoa(v.material),
      }),
  });

  async function onSubmit(v: WriteValues) {
    setSubmitError(null);
    try {
      if (mode === "edit" && v.key !== address.key) {
        // Atomic rename. Material is always supplied on submit (the
        // textarea is required), so we send it along; the backend
        // overwrites in the same step.
        await renameMut.mutateAsync({
          from_key: address.key,
          to_key: v.key,
          material: v.material,
        });
      } else {
        await writeMut.mutateAsync(v);
      }
      await qc.invalidateQueries({ queryKey: ["admin", "secrets", storeName] });
      onClose();
    } catch (e) {
      setSubmitError(describeApiError(e));
    }
  }

  const pending = writeMut.isPending || renameMut.isPending;

  return (
    <Dialog open onOpenChange={(o) => !o && onClose()}>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>
            {mode === "create"
              ? `Create ${address.slot} entry`
              : `Edit ${address.slot} entry`}
          </DialogTitle>
          <DialogDescription>
            In resource{" "}
            <code className="rounded bg-(--color-surface-1) px-1 py-0.5">
              {address.resource_id}
            </code>
            , slot{" "}
            <code className="rounded bg-(--color-surface-1) px-1 py-0.5">
              {address.slot}
            </code>
            . Material is sent over TLS, base64-encoded in transit, and
            never echoed back.
          </DialogDescription>
        </DialogHeader>

        <Form {...form}>
          <form
            onSubmit={form.handleSubmit(onSubmit)}
            className="space-y-4"
            noValidate
          >
            <FormField
              control={form.control}
              name="key"
              render={({ field }) => (
                <FormItem>
                  <FormLabel>Key</FormLabel>
                  <FormControl>
                    <Input
                      autoFocus
                      autoCapitalize="off"
                      autoCorrect="off"
                      spellCheck={false}
                      placeholder={
                        address.slot === "env"
                          ? "e.g. GH_TOKEN"
                          : address.slot === "files"
                            ? "e.g. config.yaml or nested/dir/file"
                            : "e.g. default/gh-cli.secret"
                      }
                      {...field}
                    />
                  </FormControl>
                  <FormDescription>
                    {address.slot === "env"
                      ? "Environment variable name — letters, digits, and underscores."
                      : "Relative path inside the slot. Renaming this key on edit moves the entry."}
                  </FormDescription>
                  <FormMessage />
                </FormItem>
              )}
            />
            <FormField
              control={form.control}
              name="material"
              render={({ field }) => (
                <FormItem>
                  <FormLabel>Material</FormLabel>
                  <FormControl>
                    <textarea
                      data-testid="material-input"
                      rows={4}
                      className="w-full rounded-md border border-(--color-line) bg-(--color-surface-0) p-2 font-mono text-xs text-(--color-fg)"
                      {...field}
                    />
                  </FormControl>
                  <FormMessage />
                </FormItem>
              )}
            />
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
              <Button type="submit" disabled={pending}>
                {pending
                  ? "Saving…"
                  : mode === "create"
                    ? "Create"
                    : "Save"}
              </Button>
            </DialogFooter>
          </form>
        </Form>
      </DialogContent>
    </Dialog>
  );
}

// ---------- Create resource ----------

const resourceSchema = z.object({
  resource: z
    .string()
    .min(1, "Resource name is required.")
    .refine((s) => !s.startsWith("__underwrap/"), {
      message: "Reserved names starting with __underwrap/ cannot be used.",
    })
    .refine((s) => !s.startsWith("/") && !s.endsWith("/"), {
      message: "Resource name must not start or end with a slash.",
    })
    .refine((s) => !/\/(env|files|libsecret)\//.test(`${s}/`), {
      message:
        "Resource name must not contain an env/files/libsecret slot segment.",
    }),
});
type ResourceValues = z.infer<typeof resourceSchema>;

function CreateResourceDialog({
  storeName,
  existingResources,
  onClose,
}: {
  storeName: string;
  existingResources: string[];
  onClose: () => void;
}) {
  const qc = useQueryClient();
  const form = useForm<ResourceValues>({
    resolver: zodResolver(resourceSchema),
    defaultValues: { resource: "" },
  });
  const known = useMemo(() => new Set(existingResources), [existingResources]);
  const mut = useMutation({
    mutationFn: (name: string) =>
      postResourceCreate({ store: storeName, name }),
    onSuccess: async () => {
      await qc.invalidateQueries({
        queryKey: ["admin", "resources", storeName],
      });
      onClose();
    },
    onError: (err) => {
      form.setError("resource", { message: describeApiError(err) });
    },
  });

  return (
    <Dialog open onOpenChange={(o) => !o && onClose()}>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>Create resource</DialogTitle>
          <DialogDescription>
            Add a resource record to this store. Use the{" "}
            <strong className="text-(--color-fg)">+</strong> button on each
            slot (env, files, libsecret) to populate it.
          </DialogDescription>
        </DialogHeader>
        <Form {...form}>
          <form
            onSubmit={form.handleSubmit((v) => {
              if (known.has(v.resource)) {
                form.setError("resource", {
                  message: "A resource with that name already exists.",
                });
                return;
              }
              mut.mutate(v.resource);
            })}
            className="space-y-4"
            noValidate
          >
            <FormField
              control={form.control}
              name="resource"
              render={({ field }) => (
                <FormItem>
                  <FormLabel>Resource name</FormLabel>
                  <FormControl>
                    <Input
                      autoFocus
                      autoCapitalize="off"
                      autoCorrect="off"
                      spellCheck={false}
                      placeholder="e.g. github/token"
                      data-testid="create-resource-input"
                      {...field}
                    />
                  </FormControl>
                  <FormDescription>
                    Slash-delimited path, without an{" "}
                    <code>env</code>/<code>files</code>/<code>libsecret</code>{" "}
                    slot suffix.
                  </FormDescription>
                  <FormMessage />
                </FormItem>
              )}
            />
            <DialogFooter>
              <Button type="button" variant="ghost" onClick={onClose}>
                Cancel
              </Button>
              <Button type="submit" disabled={mut.isPending}>
                {mut.isPending ? "Creating…" : "Create"}
              </Button>
            </DialogFooter>
          </form>
        </Form>
      </DialogContent>
    </Dialog>
  );
}

// ---------- Delete ----------

function DeleteSecretDialog({
  storeName,
  address,
  onClose,
}: {
  storeName: string;
  address: SecretAddress;
  onClose: () => void;
}) {
  const qc = useQueryClient();
  const [submitError, setSubmitError] = useState<string | null>(null);
  const mut = useMutation({
    mutationFn: () =>
      postSecretDelete({
        store: storeName,
        resource_id: address.resource_id,
        slot: address.slot,
        key: address.key,
      }),
    onSuccess: async () => {
      await qc.invalidateQueries({
        queryKey: ["admin", "secrets", storeName],
      });
      onClose();
    },
    onError: (err) => setSubmitError(describeApiError(err)),
  });
  return (
    <Dialog open onOpenChange={(o) => !o && onClose()}>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>Delete secret</DialogTitle>
          <DialogDescription>
            Removes{" "}
            <code>
              {address.resource_id} · {address.slot} · {address.key}
            </code>{" "}
            from <code>{storeName}</code>. This cannot be undone.
          </DialogDescription>
        </DialogHeader>
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
            disabled={mut.isPending}
            onClick={() => mut.mutate()}
          >
            {mut.isPending ? "Deleting…" : "Delete"}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}

// ---------- Rename resource ----------

const renameResourceSchema = z.object({
  new_name: z
    .string()
    .trim()
    .min(1, "New name is required.")
    .refine((s) => !s.startsWith("__underwrap"), {
      message: "Names starting with '__underwrap' are reserved.",
    }),
});
type RenameResourceValues = z.infer<typeof renameResourceSchema>;

/**
 * Rename the operator-facing label of a resource. The backend id and
 * every reference to it (pair records, wrapper permissions, on-disk
 * blob key) stay put — this is a metadata flip.
 */
function RenameResourceDialog({
  storeName,
  resourceId,
  currentName,
  existingNames,
  onClose,
}: {
  storeName: string;
  resourceId: string;
  currentName: string;
  existingNames: string[];
  onClose: () => void;
}) {
  const qc = useQueryClient();
  const form = useForm<RenameResourceValues>({
    resolver: zodResolver(renameResourceSchema),
    defaultValues: { new_name: currentName },
  });
  const otherNames = useMemo(
    () => new Set(existingNames.filter((n) => n !== currentName)),
    [existingNames, currentName],
  );
  const mut = useMutation({
    mutationFn: (new_name: string) =>
      postResourceRename({ store: storeName, id: resourceId, new_name }),
    onSuccess: async () => {
      await Promise.all([
        qc.invalidateQueries({ queryKey: ["admin", "resources", storeName] }),
        qc.invalidateQueries({ queryKey: ["admin", "pairs", storeName] }),
        qc.invalidateQueries({
          queryKey: ["admin", "wrapper-permissions", storeName],
        }),
      ]);
      onClose();
    },
    onError: (err) => {
      form.setError("new_name", { message: describeApiError(err) });
    },
  });

  return (
    <Dialog open onOpenChange={(o) => !o && onClose()}>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>Rename resource</DialogTitle>
          <DialogDescription>
            Change the operator-facing name. The backend id and every
            pair/permission/secret reference stay put — this is a
            metadata flip, not a move.
          </DialogDescription>
        </DialogHeader>
        <Form {...form}>
          <form
            onSubmit={form.handleSubmit((v) => {
              const next = v.new_name.trim();
              if (next === currentName) {
                onClose();
                return;
              }
              if (otherNames.has(next)) {
                form.setError("new_name", {
                  message: "Another resource already uses that name.",
                });
                return;
              }
              mut.mutate(next);
            })}
            className="space-y-4"
            noValidate
          >
            <FormField
              control={form.control}
              name="new_name"
              render={({ field }) => (
                <FormItem>
                  <FormLabel>New name</FormLabel>
                  <FormControl>
                    <Input
                      autoFocus
                      autoCapitalize="off"
                      autoCorrect="off"
                      spellCheck={false}
                      data-testid="rename-resource-input"
                      {...field}
                    />
                  </FormControl>
                  <FormDescription>
                    Current: <code>{currentName}</code>. Wrapper configs
                    referencing the old name will need updating.
                  </FormDescription>
                  <FormMessage />
                </FormItem>
              )}
            />
            <DialogFooter>
              <Button type="button" variant="ghost" onClick={onClose}>
                Cancel
              </Button>
              <Button type="submit" disabled={mut.isPending}>
                {mut.isPending ? "Renaming…" : "Rename"}
              </Button>
            </DialogFooter>
          </form>
        </Form>
      </DialogContent>
    </Dialog>
  );
}

// ---------- Delete resource ----------

function DeleteResourceDialog({
  storeName,
  resourceId,
  resourceName,
  onClose,
}: {
  storeName: string;
  resourceId: string;
  resourceName: string;
  onClose: () => void;
}) {
  const qc = useQueryClient();
  const [submitError, setSubmitError] = useState<string | null>(null);
  const mut = useMutation({
    mutationFn: () =>
      postResourceDelete({ store: storeName, id: resourceId }),
  });
  async function onDelete() {
    setSubmitError(null);
    try {
      const result = await mut.mutateAsync();
      if (result.ok) {
        await Promise.all([
          qc.invalidateQueries({ queryKey: ["admin", "resources", storeName] }),
          qc.invalidateQueries({ queryKey: ["admin", "secrets", storeName] }),
          qc.invalidateQueries({ queryKey: ["admin", "pairs", storeName] }),
        ]);
        onClose();
      }
      // result.ok === false → backend reported blockers; rendered below.
    } catch (e) {
      setSubmitError(describeApiError(e));
    }
  }
  const blockers = mut.data?.blocked_by;
  return (
    <Dialog open onOpenChange={(o) => !o && onClose()}>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>Delete resource: {resourceName}</DialogTitle>
          <DialogDescription>
            Removes the resource record. The backend refuses if any
            secrets, wrapper/resource pairs, or grants still reference
            this resource.
          </DialogDescription>
        </DialogHeader>
        {blockers ? (
          <div
            role="alert"
            className="space-y-2 rounded-md border border-amber-500/30 bg-amber-500/10 px-3 py-2 text-xs text-amber-200"
            data-testid="resource-delete-blockers"
          >
            <p>Cannot delete — the following still reference this resource:</p>
            <ul className="list-disc pl-5">
              {blockers.secret_count > 0 ? (
                <li>
                  <strong>{blockers.secret_count}</strong> secret
                  {blockers.secret_count === 1 ? "" : "s"}
                  {blockers.sample_secret_ids.length > 0 ? (
                    <>
                      {" "}
                      (e.g.{" "}
                      <code className="font-mono">
                        {blockers.sample_secret_ids.join(", ")}
                      </code>
                      )
                    </>
                  ) : null}
                </li>
              ) : null}
              {blockers.pair_count > 0 ? (
                <li>
                  <strong>{blockers.pair_count}</strong> wrapper/resource
                  pair{blockers.pair_count === 1 ? "" : "s"}
                  {blockers.sample_pairs.length > 0 ? (
                    <>
                      {" "}
                      (e.g.{" "}
                      <code className="font-mono">
                        {blockers.sample_pairs
                          .map((p) => `${p.wrapper}/${p.resource}`)
                          .join(", ")}
                      </code>
                      )
                    </>
                  ) : null}
                </li>
              ) : null}
              {blockers.permission_count > 0 ? (
                <li>
                  <strong>{blockers.permission_count}</strong> wrapper permission
                  {blockers.permission_count === 1 ? "" : "s"}
                </li>
              ) : null}
            </ul>
            <p className="text-amber-200/80">
              Clear those dependents first, then try again.
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
            disabled={mut.isPending}
            onClick={() => void onDelete()}
            data-testid="resource-delete-confirm"
          >
            {mut.isPending ? "Deleting…" : "Delete"}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}

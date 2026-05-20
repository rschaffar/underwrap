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
  type AdminPairsListResponse,
  type AdminResourcesListResponse,
  type AdminSecretListResponse,
  type AdminSecretReadResponse,
  type AdminStoresListResponse,
  type EntityCreator,
  fetchStores,
  postPairsList,
  postResourceCreate,
  postResourceDelete,
  postResourcesList,
  postSecretDelete,
  postSecretList,
  postSecretRead,
  postSecretWrite,
  type ResourceRecord,
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

/**
 * The three backend "slots" every wrapper resource has. The secret
 * ID layout is `<resource>/<slot>/<name>` — see `examples/config.yaml`
 * and `crates/shared-protocol/src/config.rs`.
 */
export const SECRET_SLOTS = ["env", "files", "libsecret"] as const;
export type SecretSlot = (typeof SECRET_SLOTS)[number];

export interface SecretEntry {
  /** Full secret ID, as stored in the backend. */
  id: string;
  /** Entry name within the slot — everything after `<slot>/`. */
  name: string;
}

export interface SecretResourceGroup {
  resource: string;
  env: SecretEntry[];
  files: SecretEntry[];
  libsecret: SecretEntry[];
}

/** Total entry count across all three slots of a resource. */
function resourceCount(g: SecretResourceGroup): number {
  return g.env.length + g.files.length + g.libsecret.length;
}

/**
 * Parse a list of secret IDs into resource-grouped buckets. Each
 * ID is expected to match `<resource>/<slot>/<name>` where `<slot>`
 * is one of env/files/libsecret. IDs that don't match the layout
 * are surfaced separately as `unrecognized` so the operator can
 * see and clean them up rather than have them silently disappear.
 *
 * Non-greedy on the resource segment so a deeply-nested resource
 * path resolves to the first valid slot delimiter (the common
 * shape — see the `prefer the leftmost split` test case).
 */
export function groupSecretIds(ids: string[]): {
  resources: SecretResourceGroup[];
  unrecognized: string[];
} {
  const groups = new Map<string, SecretResourceGroup>();
  const unrecognized: string[] = [];
  const pattern = /^(.+?)\/(env|files|libsecret)\/(.+)$/;
  for (const id of ids) {
    const m = pattern.exec(id);
    if (!m) {
      unrecognized.push(id);
      continue;
    }
    const [, resource, slot, name] = m as unknown as [
      string,
      string,
      SecretSlot,
      string,
    ];
    let g = groups.get(resource);
    if (!g) {
      g = { resource, env: [], files: [], libsecret: [] };
      groups.set(resource, g);
    }
    g[slot].push({ id, name });
  }
  // Stable sort within each slot, then by resource name.
  for (const g of groups.values()) {
    g.env.sort((a, b) => a.name.localeCompare(b.name));
    g.files.sort((a, b) => a.name.localeCompare(b.name));
    g.libsecret.sort((a, b) => a.name.localeCompare(b.name));
  }
  const resources = [...groups.values()].sort((a, b) =>
    a.resource.localeCompare(b.resource),
  );
  unrecognized.sort();
  return { resources, unrecognized };
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
    queryFn: () => postSecretList({ store: storeName!, prefix: "" }),
    enabled: !!storeName,
  });
  // Resources are first-class backend records — these drive the
  // "empty resource" rows (resources that exist but have no secrets
  // yet) and the provenance display. The pairs list seeds the wrapper
  // selector in the write dialog.
  const resourcesList = useQuery<AdminResourcesListResponse>({
    queryKey: ["admin", "resources", storeName],
    queryFn: () => postResourcesList({ store: storeName! }),
    enabled: !!storeName,
  });
  const pairsList = useQuery<AdminPairsListResponse>({
    queryKey: ["admin", "pairs", storeName],
    queryFn: () => postPairsList({ store: storeName! }),
    enabled: !!storeName,
  });
  const isAdmin = !!(_principal?.authenticated && _principal.role === "admin");
  const wrapperChoices = useMemo(() => {
    const set = new Set<string>();
    for (const p of pairsList.data?.pairs ?? []) set.add(p.wrapper);
    return [...set].sort();
  }, [pairsList.data]);
  const resourceProvenance = useMemo(() => {
    const map = new Map<string, ResourceRecord>();
    for (const r of resourcesList.data?.resources ?? []) {
      map.set(r.name, r);
    }
    return map;
  }, [resourcesList.data]);

  /**
   * Map from resource name to the wrappers that project its `files`
   * slot, plus the target-home-relative paths each wrapper mounts it
   * at. Used by SlotSection (slot=files) to render the resolved
   * location for each stored file.
   */
  const fileMountsByResource = useMemo(() => {
    const map = new Map<string, Array<{ wrapper: string; paths: string[] }>>();
    for (const p of pairsList.data?.pairs ?? []) {
      if (p.files_mount_paths.length === 0) continue;
      const list = map.get(p.resource) ?? [];
      list.push({ wrapper: p.wrapper, paths: p.files_mount_paths });
      map.set(p.resource, list);
    }
    for (const arr of map.values()) arr.sort((a, b) => a.wrapper.localeCompare(b.wrapper));
    return map;
  }, [pairsList.data]);

  // Filter out reserved underwrap internal secret IDs so operators
  // don't accidentally read or delete them through this UI.
  // The backend already rejects writes for the policy ID with a clear
  // error, but hiding it removes the foot-gun entirely.
  const RESERVED_PREFIX = /^__underwrap\//;
  const visibleIds = useMemo(
    () => (list.data?.ids ?? []).filter((id) => !RESERVED_PREFIX.test(id)),
    [list.data],
  );
  const grouped = useMemo(() => groupSecretIds(visibleIds), [visibleIds]);

  // Resources known to this store: every persisted resource record
  // (which lives on the backend with provenance) plus any extra
  // resources we observe in the secret list under a name not yet
  // recorded. The second case shouldn't happen in normal operation,
  // but tolerating it means a legacy import doesn't break the UI.
  const allResources = useMemo<SecretResourceGroup[]>(() => {
    const byResource = new Map<string, SecretResourceGroup>();
    for (const g of grouped.resources) {
      byResource.set(g.resource, g);
    }
    for (const record of resourcesList.data?.resources ?? []) {
      if (!byResource.has(record.name)) {
        byResource.set(record.name, {
          resource: record.name,
          env: [],
          files: [],
          libsecret: [],
        });
      }
    }
    return [...byResource.values()].sort((a, b) =>
      a.resource.localeCompare(b.resource),
    );
  }, [grouped.resources, resourcesList.data]);

  const [revealing, setRevealing] = useState<string | null>(null);
  const [editing, setEditing] = useState<string | null>(null);
  const [creating, setCreating] = useState<{ prefilledId?: string } | null>(
    null,
  );
  const [creatingResource, setCreatingResource] = useState(false);
  const [deleting, setDeleting] = useState<string | null>(null);
  const [deletingResource, setDeletingResource] = useState<string | null>(null);

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
      ) : allResources.length === 0 && grouped.unrecognized.length === 0 ? (
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
              key={group.resource}
              group={group}
              provenance={resourceProvenance.get(group.resource)}
              fileMounts={fileMountsByResource.get(group.resource) ?? []}
              isAdmin={isAdmin}
              onReveal={setRevealing}
              onEdit={setEditing}
              onDelete={setDeleting}
              onDeleteResource={() => setDeletingResource(group.resource)}
              onAddEntry={(slot) =>
                setCreating({
                  prefilledId: `${group.resource}/${slot}/`,
                })
              }
            />
          ))}
          {grouped.unrecognized.length > 0 ? (
            <UnrecognizedBlock
              ids={grouped.unrecognized}
              onReveal={setRevealing}
              onEdit={setEditing}
              onDelete={setDeleting}
            />
          ) : null}
        </div>
      )}

      {revealing ? (
        <RevealSecretDialog
          storeName={storeName}
          secretId={revealing}
          onClose={() => setRevealing(null)}
        />
      ) : null}
      {creating ? (
        <WriteSecretDialog
          storeName={storeName}
          mode="create"
          prefilledSecretId={creating.prefilledId}
          wrapperChoices={wrapperChoices}
          onClose={() => setCreating(null)}
        />
      ) : null}
      {creatingResource ? (
        <CreateResourceDialog
          storeName={storeName}
          existingResources={allResources.map((r) => r.resource)}
          onClose={() => setCreatingResource(false)}
        />
      ) : null}
      {editing ? (
        <WriteSecretDialog
          storeName={storeName}
          mode="edit"
          existingId={editing}
          wrapperChoices={wrapperChoices}
          onClose={() => setEditing(null)}
        />
      ) : null}
      {deleting ? (
        <DeleteSecretDialog
          storeName={storeName}
          secretId={deleting}
          onClose={() => setDeleting(null)}
        />
      ) : null}
      {deletingResource ? (
        <DeleteResourceDialog
          storeName={storeName}
          resourceName={deletingResource}
          onClose={() => setDeletingResource(null)}
        />
      ) : null}
    </div>
  );
}

// ---------- Resource block (expandable) ----------

interface RowActions {
  onReveal: (id: string) => void;
  onEdit: (id: string) => void;
  onDelete: (id: string) => void;
}

function ResourceBlock({
  group,
  provenance,
  fileMounts,
  isAdmin,
  onAddEntry,
  onDeleteResource,
  ...actions
}: {
  group: SecretResourceGroup;
  provenance: ResourceRecord | undefined;
  fileMounts: Array<{ wrapper: string; paths: string[] }>;
  isAdmin: boolean;
  onAddEntry: (slot: SecretSlot) => void;
  onDeleteResource: () => void;
} & RowActions) {
  const total = resourceCount(group);
  // The summary line shows which slots have content. It avoids
  // overpromising an empty slot section before the operator
  // expands.
  const summary = SECRET_SLOTS.filter((slot) => group[slot].length > 0)
    .map((slot) => `${slot} (${group[slot].length})`)
    .join(" · ");
  const provenanceLabel = provenance ? describeCreator(provenance.created_by) : null;
  const provenanceDate =
    provenance && Number(provenance.created_at)
      ? new Date(Number(provenance.created_at) * 1000)
      : null;
  return (
    <details
      data-testid={`resource-block-${group.resource}`}
      className="group rounded-md border border-(--color-line) bg-(--color-surface-0)/40 open:bg-(--color-surface-0)/70"
    >
      <summary className="flex cursor-pointer list-none items-center gap-2 px-4 py-3 text-sm">
        <ChevronRight
          className="h-4 w-4 transition-transform group-open:rotate-90"
          aria-hidden
        />
        <span className="font-mono text-(--color-fg)">{group.resource}</span>
        {provenanceLabel ? (
          <span
            className="text-xs text-(--color-muted)"
            title={
              provenanceDate
                ? `Created ${provenanceDate.toISOString()} by ${provenanceLabel}`
                : undefined
            }
            data-testid={`resource-provenance-${group.resource}`}
          >
            · by {provenanceLabel}
            {provenanceDate ? ` on ${provenanceDate.toLocaleDateString()}` : null}
          </span>
        ) : null}
        <span className="ml-auto text-xs text-(--color-muted)">
          {total === 0 ? "empty" : summary}
        </span>
        {isAdmin && provenance ? (
          <Button
            type="button"
            variant="ghost"
            size="sm"
            aria-label={`Delete resource ${group.resource}`}
            data-testid={`resource-delete-${group.resource}`}
            onClick={(e) => {
              e.preventDefault();
              e.stopPropagation();
              onDeleteResource();
            }}
            className="h-7 px-2"
          >
            <Trash2 className="h-3.5 w-3.5" aria-hidden />
          </Button>
        ) : null}
      </summary>
      <div className="space-y-3 border-t border-(--color-line)/60 px-4 py-3">
        {SECRET_SLOTS.map((slot) => (
          <SlotSection
            key={slot}
            slot={slot}
            entries={group[slot]}
            fileMounts={slot === "files" ? fileMounts : []}
            isAdmin={isAdmin}
            onAdd={() => onAddEntry(slot)}
            {...actions}
          />
        ))}
      </div>
    </details>
  );
}

function describeCreator(creator: EntityCreator): string {
  switch (creator.kind) {
    case "operator":
      return creator.username;
    case "client-instance":
      return `client ${creator.id}`;
  }
}

function SlotSection({
  slot,
  entries,
  fileMounts,
  isAdmin: _isAdmin,
  onAdd,
  ...actions
}: {
  slot: SecretSlot;
  entries: SecretEntry[];
  fileMounts: Array<{ wrapper: string; paths: string[] }>;
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
              key={entry.id}
              className="flex flex-col gap-1 rounded-md px-2 py-1.5 hover:bg-(--color-surface-1)/50 sm:flex-row sm:items-center sm:gap-2"
            >
              <div className="flex min-w-0 flex-col gap-0.5">
                <span
                  className="font-mono text-xs text-(--color-fg)"
                  title={entry.id}
                >
                  {entry.name}
                </span>
                {slot === "files" && fileMounts.length > 0 ? (
                  <span
                    className="font-mono text-[10px] text-(--color-muted)"
                    data-testid={`file-mounts-${entry.id}`}
                  >
                    {fileMounts
                      .flatMap((m) =>
                        m.paths.map((p) => `${p}/${entry.name} (${m.wrapper})`),
                      )
                      .join(" · ")}
                  </span>
                ) : null}
              </div>
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
      <Button variant="ghost" size="sm" onClick={() => onReveal(entry.id)}>
        <Eye className="mr-1.5 h-4 w-4" aria-hidden />
        Reveal
      </Button>
      <Button
        variant="ghost"
        size="sm"
        onClick={() => onEdit(entry.id)}
      >
        <Pencil className="mr-1.5 h-4 w-4" aria-hidden />
        Edit
      </Button>
      <Button
        variant="destructive"
        size="sm"
        onClick={() => onDelete(entry.id)}
      >
        <Trash2 className="mr-1.5 h-4 w-4" aria-hidden />
        Delete
      </Button>
    </>
  );
}

function UnrecognizedBlock({
  ids,
  ...actions
}: { ids: string[] } & RowActions) {
  return (
    <details
      data-testid="resource-block-unrecognized"
      className="group rounded-md border border-amber-500/30 bg-amber-500/5"
    >
      <summary className="flex cursor-pointer list-none items-center gap-2 px-4 py-3 text-sm">
        <ChevronRight
          className="h-4 w-4 transition-transform group-open:rotate-90"
          aria-hidden
        />
        <span className="text-amber-200">Unrecognized layout</span>
        <span className="ml-auto text-xs text-amber-200/70">
          {ids.length} {ids.length === 1 ? "secret" : "secrets"}
        </span>
      </summary>
      <div className="space-y-1 border-t border-amber-500/30 px-4 py-3">
        <p className="pb-2 text-xs text-amber-200/80">
          These IDs don't follow the{" "}
          <code className="rounded bg-(--color-surface-1) px-1 py-0.5">
            &lt;resource&gt;/&lt;env|files|libsecret&gt;/&lt;name&gt;
          </code>{" "}
          convention. They may be left over from an older import.
        </p>
        <ul className="space-y-1">
          {ids.map((id) => (
            <li
              key={id}
              className="flex items-center gap-2 rounded-md px-2 py-1.5 hover:bg-(--color-surface-1)/50"
            >
              <span className="font-mono text-xs text-(--color-fg)">{id}</span>
              <div className="ml-auto inline-flex gap-2">
                <EntryActions entry={{ id, name: id }} {...actions} />
              </div>
            </li>
          ))}
        </ul>
      </div>
    </details>
  );
}

// ---------- Reveal ----------

function RevealSecretDialog({
  storeName,
  secretId,
  onClose,
}: {
  storeName: string;
  secretId: string;
  onClose: () => void;
}) {
  const [confirmed, setConfirmed] = useState(false);
  const [copied, setCopied] = useState(false);
  const read = useQuery<AdminSecretReadResponse>({
    queryKey: ["admin", "secret", "read", storeName, secretId],
    queryFn: () => postSecretRead({ store: storeName, secret_id: secretId }),
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
            <code>{secretId}</code> from <code>{storeName}</code>.
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

const writeSchema = z.object({
  secret_id: z
    .string()
    .min(1, "Secret ID is required.")
    .refine((s) => !s.startsWith("__underwrap/"), {
      message: "Reserved IDs starting with __underwrap/ cannot be used.",
    }),
  wrapper: z.string().min(1, "Pick a wrapper context."),
  material: z.string().min(1, "Material is required."),
});
type WriteValues = z.infer<typeof writeSchema>;

function WriteSecretDialog({
  storeName,
  mode,
  existingId,
  prefilledSecretId,
  wrapperChoices,
  onClose,
}: {
  storeName: string;
  mode: "create" | "edit";
  existingId?: string;
  prefilledSecretId?: string;
  wrapperChoices: string[];
  onClose: () => void;
}) {
  const qc = useQueryClient();
  const [submitError, setSubmitError] = useState<string | null>(null);
  const form = useForm<WriteValues>({
    resolver: zodResolver(writeSchema),
    defaultValues: {
      secret_id: existingId ?? prefilledSecretId ?? "",
      wrapper: wrapperChoices[0] ?? "",
      material: "",
    },
  });

  const mut = useMutation({
    mutationFn: (v: WriteValues) =>
      postSecretWrite({
        store: storeName,
        wrapper: v.wrapper,
        secret_id: v.secret_id,
        material_b64: btoa(v.material),
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
          <DialogTitle>
            {mode === "create" ? "Create secret" : `Edit: ${existingId}`}
          </DialogTitle>
          <DialogDescription>
            {mode === "create"
              ? "New secret in this store."
              : "Overwrite the stored material. Old value cannot be recovered."}{" "}
            Material is sent over TLS, base64-encoded in transit, and
            never echoed back.
          </DialogDescription>
        </DialogHeader>

        <Form {...form}>
          <form
            onSubmit={form.handleSubmit((v) => {
              setSubmitError(null);
              mut.mutate(v);
            })}
            className="space-y-4"
            noValidate
          >
            <FormField
              control={form.control}
              name="secret_id"
              render={({ field }) => (
                <FormItem>
                  <FormLabel>Secret ID</FormLabel>
                  <FormControl>
                    <Input
                      autoFocus={mode === "create"}
                      readOnly={mode === "edit"}
                      autoCapitalize="off"
                      autoCorrect="off"
                      spellCheck={false}
                      placeholder="e.g. github/token/env/GH_TOKEN"
                      {...field}
                    />
                  </FormControl>
                  <FormDescription>
                    Slash-delimited path. <code>env/&lt;NAME&gt;</code>{" "}
                    suffixes are conventional for environment-mode
                    wrappers.
                  </FormDescription>
                  <FormMessage />
                </FormItem>
              )}
            />
            <FormField
              control={form.control}
              name="wrapper"
              render={({ field }) => (
                <FormItem>
                  <FormLabel>Wrapper context</FormLabel>
                  <FormControl>
                    {wrapperChoices.length === 0 ? (
                      <Input
                        autoCapitalize="off"
                        autoCorrect="off"
                        spellCheck={false}
                        placeholder="e.g. gh-authorized"
                        {...field}
                      />
                    ) : (
                      <select
                        {...field}
                        className="flex h-9 w-full rounded-md border border-(--color-line) bg-(--color-surface-0) px-3 py-2 text-sm text-(--color-fg)"
                      >
                        {wrapperChoices.map((w) => (
                          <option key={w} value={w}>
                            {w}
                          </option>
                        ))}
                      </select>
                    )}
                  </FormControl>
                  <FormDescription>
                    Used for backend write authorization. Free-form
                    when no wrapper/resource pairs are registered yet.
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
              <Button type="submit" disabled={mut.isPending}>
                {mut.isPending
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
  secretId,
  onClose,
}: {
  storeName: string;
  secretId: string;
  onClose: () => void;
}) {
  const qc = useQueryClient();
  const [submitError, setSubmitError] = useState<string | null>(null);
  const mut = useMutation({
    mutationFn: () =>
      postSecretDelete({ store: storeName, secret_id: secretId }),
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
            Removes <code>{secretId}</code> from <code>{storeName}</code>.
            This cannot be undone.
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

// ---------- Delete resource ----------

function DeleteResourceDialog({
  storeName,
  resourceName,
  onClose,
}: {
  storeName: string;
  resourceName: string;
  onClose: () => void;
}) {
  const qc = useQueryClient();
  const [submitError, setSubmitError] = useState<string | null>(null);
  const mut = useMutation({
    mutationFn: () =>
      postResourceDelete({ store: storeName, name: resourceName }),
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

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
  type AdminCatalogGetResponse,
  type AdminSecretListResponse,
  type AdminSecretReadResponse,
  type AdminStoresListResponse,
  type AdminWritePermissionsResponse,
  fetchStores,
  postCatalogGet,
  postSecretDelete,
  postSecretList,
  postSecretRead,
  postSecretWrite,
  postWritePermissionsGet,
  postWritePermissionsSet,
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

  // Store selector — URL-persisted, same shape as the Matrix page.
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
  // Catalog gives us the set of wrappers known to this store, which
  // we use to populate the wrapper selector in the write dialog.
  const catalog = useQuery<AdminCatalogGetResponse>({
    queryKey: ["admin", "catalog", storeName],
    queryFn: () => postCatalogGet({ store: storeName! }),
    enabled: !!storeName,
  });
  // Per-resource wrapper-write permissions. Admin writes through the
  // SPA are unconditional, but the toggles surface what the wrapper
  // can do — admins flip them; operators see the state but cannot
  // change it.
  const permissions = useQuery<AdminWritePermissionsResponse>({
    queryKey: ["admin", "write-permissions", storeName],
    queryFn: () => postWritePermissionsGet({ store: storeName! }),
    enabled: !!storeName,
  });
  const isAdmin = !!(_principal?.authenticated && _principal.role === "admin");
  const wrapperChoices = useMemo(() => {
    const set = new Set<string>();
    for (const e of catalog.data?.entries ?? []) set.add(e.wrapper);
    return [...set].sort();
  }, [catalog.data]);

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

  // Resources known to this store from any source: actually-stored
  // secrets, catalog entries from wrappers that have registered, and
  // every resource that already has a wrapper-write permission set.
  // Listing all of them — even when they hold zero secrets — is what
  // lets an admin pre-grant `files`/`libsecret` writes BEFORE the
  // wrapper writes its first entry. Without this, the page would
  // hide an empty resource and the wrapper would never be able to
  // initialise it.
  const allResources = useMemo<SecretResourceGroup[]>(() => {
    const byResource = new Map<string, SecretResourceGroup>();
    for (const g of grouped.resources) {
      byResource.set(g.resource, g);
    }
    const ensureEmpty = (resource: string) => {
      if (!byResource.has(resource)) {
        byResource.set(resource, {
          resource,
          env: [],
          files: [],
          libsecret: [],
        });
      }
    };
    for (const entry of catalog.data?.entries ?? []) {
      ensureEmpty(entry.resource);
    }
    for (const r of Object.keys(permissions.data?.files_writable ?? {})) {
      ensureEmpty(r);
    }
    for (const r of Object.keys(permissions.data?.libsecret_writable ?? {})) {
      ensureEmpty(r);
    }
    return [...byResource.values()].sort((a, b) =>
      a.resource.localeCompare(b.resource),
    );
  }, [grouped.resources, catalog.data, permissions.data]);

  const [revealing, setRevealing] = useState<string | null>(null);
  const [editing, setEditing] = useState<string | null>(null);
  const [creating, setCreating] = useState(false);
  const [deleting, setDeleting] = useState<string | null>(null);

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
            onClick={() => setCreating(true)}
          >
            <Plus className="mr-1.5 h-4 w-4" aria-hidden />
            Create secret
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
          <strong className="text-(--color-fg)">Create secret</strong>{" "}
          to add one, or wait for a wrapper to register its catalog.
        </p>
      ) : (
        <div className="space-y-2">
          {allResources.map((group) => (
            <ResourceBlock
              key={group.resource}
              storeName={storeName}
              group={group}
              filesWritable={
                permissions.data?.files_writable[group.resource] ?? false
              }
              libsecretWritable={
                permissions.data?.libsecret_writable[group.resource] ?? false
              }
              isAdmin={isAdmin}
              onReveal={setRevealing}
              onEdit={setEditing}
              onDelete={setDeleting}
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
          wrapperChoices={wrapperChoices}
          onClose={() => setCreating(false)}
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
  storeName,
  group,
  filesWritable,
  libsecretWritable,
  isAdmin,
  ...actions
}: {
  storeName: string;
  group: SecretResourceGroup;
  filesWritable: boolean;
  libsecretWritable: boolean;
  isAdmin: boolean;
} & RowActions) {
  const total = resourceCount(group);
  // The summary line shows which slots have content. It avoids
  // overpromising an empty slot section before the operator
  // expands.
  const summary = SECRET_SLOTS.filter((slot) => group[slot].length > 0)
    .map((slot) => `${slot} (${group[slot].length})`)
    .join(" · ");
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
        <span className="ml-auto text-xs text-(--color-muted)">
          {total === 0 ? "empty" : summary}
        </span>
      </summary>
      <div className="space-y-3 border-t border-(--color-line)/60 px-4 py-3">
        <SlotSection
          slot="env"
          entries={group.env}
          wrapperWritable={null}
          storeName={storeName}
          resource={group.resource}
          isAdmin={isAdmin}
          {...actions}
        />
        <SlotSection
          slot="files"
          entries={group.files}
          wrapperWritable={filesWritable}
          storeName={storeName}
          resource={group.resource}
          isAdmin={isAdmin}
          {...actions}
        />
        <SlotSection
          slot="libsecret"
          entries={group.libsecret}
          wrapperWritable={libsecretWritable}
          storeName={storeName}
          resource={group.resource}
          isAdmin={isAdmin}
          {...actions}
        />
      </div>
    </details>
  );
}

function SlotSection({
  slot,
  entries,
  wrapperWritable,
  storeName,
  resource,
  isAdmin,
  ...actions
}: {
  slot: SecretSlot;
  entries: SecretEntry[];
  /** `null` for the `env` slot — env has no wrapper-side write path. */
  wrapperWritable: boolean | null;
  storeName: string;
  resource: string;
  isAdmin: boolean;
} & RowActions) {
  return (
    <section data-testid={`slot-${slot}`} className="space-y-1">
      <div className="flex items-center gap-2">
        <h3 className="font-mono text-xs uppercase tracking-wide text-(--color-muted)">
          {slot}
        </h3>
        {wrapperWritable === null ? null : (
          <WrapperWritableToggle
            slot={slot as "files" | "libsecret"}
            storeName={storeName}
            resource={resource}
            value={wrapperWritable}
            isAdmin={isAdmin}
          />
        )}
      </div>
      {entries.length === 0 ? (
        <p className="pl-2 text-xs text-(--color-muted)/70">(none)</p>
      ) : (
        <ul className="space-y-1">
          {entries.map((entry) => (
            <li
              key={entry.id}
              className="flex items-center gap-2 rounded-md px-2 py-1.5 hover:bg-(--color-surface-1)/50"
            >
              <span
                className="font-mono text-xs text-(--color-fg)"
                title={entry.id}
              >
                {entry.name}
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

function WrapperWritableToggle({
  slot,
  storeName,
  resource,
  value,
  isAdmin,
}: {
  slot: "files" | "libsecret";
  storeName: string;
  resource: string;
  value: boolean;
  isAdmin: boolean;
}) {
  const qc = useQueryClient();
  const mut = useMutation({
    mutationFn: (enabled: boolean) =>
      postWritePermissionsSet({
        store: storeName,
        resource,
        slot,
        enabled,
      }),
    onSuccess: () =>
      qc.invalidateQueries({
        queryKey: ["admin", "write-permissions", storeName],
      }),
  });
  return (
    <label
      className="inline-flex items-center gap-1.5 text-xs text-(--color-muted)"
      title={
        isAdmin
          ? `Allow wrapper-side writes to ${slot} entries of this resource`
          : "Only admins can change wrapper write permissions"
      }
    >
      <input
        type="checkbox"
        data-testid={`wrapper-writable-${slot}-${resource}`}
        checked={value}
        disabled={!isAdmin || mut.isPending}
        onChange={(e) => mut.mutate(e.target.checked)}
      />
      <span>wrapper writable</span>
    </label>
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
  wrapperChoices,
  onClose,
}: {
  storeName: string;
  mode: "create" | "edit";
  existingId?: string;
  wrapperChoices: string[];
  onClose: () => void;
}) {
  const qc = useQueryClient();
  const [submitError, setSubmitError] = useState<string | null>(null);
  const form = useForm<WriteValues>({
    resolver: zodResolver(writeSchema),
    defaultValues: {
      secret_id: existingId ?? "",
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
                    when the catalog is empty.
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
  const [confirmText, setConfirmText] = useState("");
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
  const canDelete = confirmText === secretId && !mut.isPending;
  return (
    <Dialog open onOpenChange={(o) => !o && onClose()}>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>Delete secret</DialogTitle>
          <DialogDescription>
            Removes <code>{secretId}</code> from <code>{storeName}</code>.
            Type the secret ID to confirm.
          </DialogDescription>
        </DialogHeader>
        <div className="space-y-3">
          <div className="space-y-1.5">
            <Label htmlFor="confirm-secret-id">
              Type{" "}
              <code className="rounded bg-(--color-surface-1) px-1 py-0.5">
                {secretId}
              </code>{" "}
              to confirm
            </Label>
            <Input
              id="confirm-secret-id"
              value={confirmText}
              onChange={(e) => setConfirmText(e.target.value)}
              autoFocus
              autoComplete="off"
              autoCapitalize="off"
              autoCorrect="off"
              spellCheck={false}
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
        <DialogFooter>
          <Button type="button" variant="ghost" onClick={onClose}>
            Cancel
          </Button>
          <Button
            type="button"
            variant="destructive"
            disabled={!canDelete}
            onClick={() => mut.mutate()}
          >
            {mut.isPending ? "Deleting…" : "Delete"}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}

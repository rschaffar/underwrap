import { type ReactNode, useEffect, useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useForm } from "react-hook-form";
import { zodResolver } from "@hookform/resolvers/zod";
import { z } from "zod";
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
  FormField,
  FormItem,
  FormLabel,
  FormMessage,
} from "@/components/ui/form";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import {
  type AdminStoreCreateRequest,
  type AdminStoreDetailResponse,
  type AdminStoresListResponse,
  deleteStore,
  fetchStores,
  postStoreCreate,
  postStoreRotateKey,
  putStoreUpdate,
  type WhoamiResponse,
} from "@/api/client";
import { describeApiError } from "@/lib/error-message";

interface StoresPageProps {
  /** Whoami payload from the root router context — drives admin-gating. */
  principal: WhoamiResponse | null;
}

/** True when the current principal is allowed to mutate stores. */
export function canMutateStores(p: WhoamiResponse | null): boolean {
  return !!(p && p.authenticated && p.role === "admin");
}

function toStoreDetail(
  item: AdminStoresListResponse["items"][number],
): AdminStoreDetailResponse {
  return {
    id: item.id,
    name: item.name,
    backend: item.backend,
    key_file: undefined,
    description: item.description,
    created_at: item.created_at,
    dir: "",
    source_path: "",
    usage: item.usage,
    actions: item.actions,
  };
}

function storeEditDisabledTitle(
  item: AdminStoresListResponse["items"][number],
  isAdmin: boolean,
): string | undefined {
  if (!isAdmin) return "Only admins may manage stores.";
  if (!item.actions.can_edit_metadata) {
    return "Store metadata cannot currently be edited.";
  }
  return undefined;
}

function storeDeleteTitle(
  item: AdminStoresListResponse["items"][number],
  isAdmin: boolean,
): string | undefined {
  if (!isAdmin) return "Only admins may manage stores.";
  if (item.actions.can_delete_now) return undefined;
  if (item.actions.can_force_delete) {
    return "Store contains secrets — a force delete confirmation is required.";
  }
  if (item.usage.mapped_client_instance_count > 0) {
    const count = item.usage.mapped_client_instance_count;
    return `Store is still mapped to ${count} client instance${count === 1 ? "" : "s"}. Remap them first.`;
  }
  return "Store cannot currently be deleted.";
}

export function StoresPage({ principal }: StoresPageProps) {
  const isAdmin = canMutateStores(principal);
  useEffect(() => {
    document.title = "underwrap — stores";
  }, []);

  const { data, error, isLoading } = useQuery<AdminStoresListResponse>({
    queryKey: ["admin", "stores"],
    queryFn: fetchStores,
  });

  const [createOpen, setCreateOpen] = useState(false);
  const [editing, setEditing] = useState<AdminStoreDetailResponse | null>(null);
  const [deleting, setDeleting] = useState<AdminStoreDetailResponse | null>(
    null,
  );

  return (
    <div className="space-y-6">
      <header className="flex flex-wrap items-baseline justify-between gap-3">
        <div>
          <h2 className="text-xl font-semibold text-(--color-fg)">Stores</h2>
          <p className="mt-1 text-sm text-(--color-muted)">
            Stores are backend-managed records. Create, edit, and delete
            them here.
          </p>
        </div>
        {isAdmin ? (
          <Button onClick={() => setCreateOpen(true)}>Create store</Button>
        ) : null}
      </header>

      {isLoading ? (
        <p className="text-sm text-(--color-muted)">Loading…</p>
      ) : error ? (
        <p role="alert" className="text-sm text-rose-300">
          {describeApiError(error)}
        </p>
      ) : (
        <StoresTable
          items={data?.items ?? []}
          isAdmin={isAdmin}
          onEdit={(item) => setEditing(toStoreDetail(item))}
          onDelete={(item) => setDeleting(toStoreDetail(item))}
        />
      )}

      {isAdmin && createOpen ? (
        <CreateStoreDialog
          open={createOpen}
          onClose={() => setCreateOpen(false)}
        />
      ) : null}
      {isAdmin && editing ? (
        <EditStoreDialog
          store={editing}
          onClose={() => setEditing(null)}
        />
      ) : null}
      {isAdmin && deleting ? (
        <DeleteStoreDialog
          store={deleting}
          onClose={() => setDeleting(null)}
        />
      ) : null}
    </div>
  );
}

interface StoresTableProps {
  items: AdminStoresListResponse["items"];
  isAdmin: boolean;
  onEdit: (item: AdminStoresListResponse["items"][number]) => void;
  onDelete: (item: AdminStoresListResponse["items"][number]) => void;
}

function StoresTable({ items, isAdmin, onEdit, onDelete }: StoresTableProps) {
  if (items.length === 0) {
    return (
      <p className="text-sm text-(--color-muted)">No stores configured.</p>
    );
  }
  return (
    <div className="overflow-x-auto rounded-xl border border-(--color-line)">
      <table className="w-full text-left text-sm">
        <thead className="bg-(--color-surface-0)/60 text-xs uppercase tracking-wide text-(--color-muted)">
          <tr>
            <th className="px-4 py-2">Name</th>
            <th className="px-4 py-2">Backend</th>
            <th className="px-4 py-2">Usage</th>
            <th className="px-4 py-2">Description</th>
            <th className="px-4 py-2 text-right">Actions</th>
          </tr>
        </thead>
        <tbody>
          {items.map((item) => {
            const canEdit = isAdmin && item.actions.can_edit_metadata;
            const canDelete =
              isAdmin && (item.actions.can_delete_now || item.actions.can_force_delete);
            return (
              <tr
                key={item.id}
                className="border-t border-(--color-line) align-middle"
              >
                <td className="px-4 py-3 font-medium text-(--color-fg)">
                  <div>{item.name}</div>
                  <div className="text-xs text-(--color-muted)">{item.id}</div>
                </td>
                <td className="px-4 py-3 text-(--color-muted)">
                  {item.backend}
                </td>
                <td className="px-4 py-3 text-(--color-muted)">
                  <div>
                    {item.usage.mapped_client_instance_count} client
                    {item.usage.mapped_client_instance_count === 1 ? "" : "s"}
                  </div>
                  <div className="text-xs">
                    {item.usage.has_secrets ? "contains secrets" : "empty"}
                  </div>
                </td>
                <td className="px-4 py-3 text-(--color-muted)">
                  {item.description ?? "—"}
                </td>
                <td className="px-4 py-3 text-right">
                  <div className="inline-flex gap-2">
                    <Button
                      variant="ghost"
                      size="sm"
                      disabled={!canEdit}
                      title={storeEditDisabledTitle(item, isAdmin)}
                      onClick={() => onEdit(item)}
                    >
                      Edit
                    </Button>
                    <Button
                      variant="destructive"
                      size="sm"
                      disabled={!canDelete}
                      title={storeDeleteTitle(item, isAdmin)}
                      onClick={() => onDelete(item)}
                    >
                      Delete
                    </Button>
                  </div>
                </td>
              </tr>
            );
          })}
        </tbody>
      </table>
    </div>
  );
}

// ---------- Create ----------

const createSchema = z.object({
  name: z
    .string()
    .trim()
    .min(1, "Store name is required."),
  backend: z.enum(["plain", "encrypted"]),
  description: z.string().optional(),
});
type CreateValues = z.infer<typeof createSchema>;

function CreateStoreDialog({
  open,
  onClose,
}: {
  open: boolean;
  onClose: () => void;
}) {
  const qc = useQueryClient();
  const [submitError, setSubmitError] = useState<string | null>(null);
  // Held only while the reveal modal is up; cleared when the operator
  // confirms acknowledgement and we close the modal. Never persisted
  // beyond React state — no localStorage / sessionStorage / URL.
  const [revealKey, setRevealKey] = useState<{
    storeName: string;
    keyHex: string;
  } | null>(null);
  const form = useForm<CreateValues>({
    resolver: zodResolver(createSchema),
    defaultValues: {
      name: "",
      backend: "plain",
      description: "",
    },
  });

  const create = useMutation({
    mutationFn: (body: AdminStoreCreateRequest) => postStoreCreate(body),
    onSuccess: async (res) => {
      await qc.invalidateQueries({ queryKey: ["admin", "stores"] });
      if (res.key_hex) {
        // Encrypted store: surface the master key once. We keep the
        // create dialog mounted underneath so onClose isn't called
        // until the operator finishes the reveal flow.
        setRevealKey({ storeName: res.store.name, keyHex: res.key_hex });
      } else {
        onClose();
      }
    },
    onError: (err) => setSubmitError(describeApiError(err)),
  });

  function onSubmit(values: CreateValues) {
    setSubmitError(null);
    const body: AdminStoreCreateRequest = {
      name: values.name,
      backend: values.backend,
    };
    if (values.description) body.description = values.description;
    create.mutate(body);
  }

  if (revealKey) {
    return (
      <KeyRevealDialog
        title={`Encryption key for ${revealKey.storeName}`}
        description={
          <>
            This is the only time the master key will be shown. Copy and
            store it somewhere safe (a password manager, an offline
            vault). The backend keeps the key file readable only by its
            own user; you do not need a copy to operate the store —
            keeping one is just an escape hatch for disaster recovery.
          </>
        }
        keyHex={revealKey.keyHex}
        onClose={() => {
          setRevealKey(null);
          onClose();
        }}
      />
    );
  }

  return (
    <Dialog open={open} onOpenChange={(o) => !o && onClose()}>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>Create store</DialogTitle>
          <DialogDescription>
            The backend allocates the on-disk location under its configured
            <code className="mx-1 rounded bg-(--color-surface-1) px-1 py-0.5 text-xs">stores_data_root</code>.
          </DialogDescription>
        </DialogHeader>
        <Form {...form}>
          <form onSubmit={form.handleSubmit(onSubmit)} className="space-y-4" noValidate>
            <FormField
              control={form.control}
              name="name"
              render={({ field }) => (
                <FormItem>
                  <FormLabel>Name</FormLabel>
                  <FormControl>
                    <Input autoFocus autoCapitalize="off" autoCorrect="off" spellCheck={false} {...field} />
                  </FormControl>
                  <FormMessage />
                </FormItem>
              )}
            />
            <FormField
              control={form.control}
              name="backend"
              render={({ field }) => (
                <FormItem>
                  <FormLabel>Backend</FormLabel>
                  <FormControl>
                    <select
                      {...field}
                      className="flex h-9 w-full rounded-md border border-(--color-line) bg-(--color-surface-0) px-3 py-2 text-sm text-(--color-fg)"
                    >
                      <option value="plain">plain</option>
                      <option value="encrypted">encrypted</option>
                    </select>
                  </FormControl>
                  <FormMessage />
                </FormItem>
              )}
            />
            {form.watch("backend") === "encrypted" ? (
              <p className="rounded-md border border-amber-500/30 bg-amber-500/10 px-3 py-2 text-xs text-amber-200">
                The backend will generate a fresh 32-byte master key,
                write it to its keys directory with mode 0600, and show
                it to you once after creation. Save it somewhere safe —
                it cannot be retrieved afterwards.
              </p>
            ) : null}
            <FormField
              control={form.control}
              name="description"
              render={({ field }) => (
                <FormItem>
                  <FormLabel>Description (optional)</FormLabel>
                  <FormControl>
                    <Input {...field} />
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
              <Button type="submit" disabled={create.isPending}>
                {create.isPending ? "Creating…" : "Create"}
              </Button>
            </DialogFooter>
          </form>
        </Form>
      </DialogContent>
    </Dialog>
  );
}

// ---------- Edit ----------

const editSchema = z.object({ description: z.string() });
type EditValues = z.infer<typeof editSchema>;

function EditStoreDialog({
  store,
  onClose,
}: {
  store: AdminStoreDetailResponse;
  onClose: () => void;
}) {
  const qc = useQueryClient();
  const [submitError, setSubmitError] = useState<string | null>(null);
  const [rotateConfirm, setRotateConfirm] = useState(false);
  const [rotateWarning, setRotateWarning] = useState<string | null>(null);
  const [revealKey, setRevealKey] = useState<{
    storeName: string;
    keyHex: string;
    warning: string;
  } | null>(null);
  const form = useForm<EditValues>({
    resolver: zodResolver(editSchema),
    defaultValues: { description: store.description ?? "" },
  });
  const mut = useMutation({
    mutationFn: (description: string) =>
      putStoreUpdate(store.id, { name: undefined, description }),
    onSuccess: async () => {
      await qc.invalidateQueries({ queryKey: ["admin", "stores"] });
      onClose();
    },
    onError: (err) => setSubmitError(describeApiError(err)),
  });
  const rotate = useMutation({
    mutationFn: () => postStoreRotateKey(store.id),
    onSuccess: (res) => {
      setRotateWarning(null);
      setRevealKey({
        storeName: store.name,
        keyHex: res.key_hex,
        warning: res.warning,
      });
    },
    onError: (err) => setRotateWarning(describeApiError(err)),
  });

  if (revealKey) {
    return (
      <KeyRevealDialog
        title={`New encryption key for ${revealKey.storeName}`}
        description={
          <>
            <strong>Heads up:</strong> {revealKey.warning} This is the
            only time the new key will be shown.
          </>
        }
        keyHex={revealKey.keyHex}
        onClose={() => {
          setRevealKey(null);
          onClose();
        }}
      />
    );
  }

  const canRotate = store.actions.can_rotate_key;

  return (
    <Dialog open onOpenChange={(o) => !o && onClose()}>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>Edit store: {store.name}</DialogTitle>
          <DialogDescription>
            Backend type and data directory are immutable. Only the
            description can be edited here. Encrypted stores also
            support master-key rotation.
          </DialogDescription>
        </DialogHeader>
        <Form {...form}>
          <form
            onSubmit={form.handleSubmit((v) => {
              setSubmitError(null);
              mut.mutate(v.description);
            })}
            className="space-y-4"
            noValidate
          >
            <FormField
              control={form.control}
              name="description"
              render={({ field }) => (
                <FormItem>
                  <FormLabel>Description</FormLabel>
                  <FormControl>
                    <Input autoFocus {...field} />
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
                {mut.isPending ? "Saving…" : "Save"}
              </Button>
            </DialogFooter>
          </form>
        </Form>
        {canRotate ? (
          <div className="mt-6 space-y-3 rounded-md border border-(--color-line) bg-(--color-surface-1) p-4">
            <h3 className="text-sm font-semibold text-(--color-fg)">
              Rotate master key
            </h3>
            <p className="text-xs text-(--color-muted)">
              Generates a fresh 32-byte master key and replaces the
              existing key file. Secrets already stored under this
              store will no longer be decryptable through this backend
              — re-encrypt them offline first if you need to keep them.
            </p>
            <label className="flex items-start gap-2 text-xs text-(--color-muted)">
              <input
                type="checkbox"
                checked={rotateConfirm}
                onChange={(e) => setRotateConfirm(e.target.checked)}
              />
              <span>
                I understand existing secrets will become unreadable
                through this backend.
              </span>
            </label>
            {rotateWarning ? (
              <p
                role="alert"
                className="rounded-md border border-rose-500/30 bg-rose-500/10 px-3 py-2 text-xs text-rose-300"
              >
                {rotateWarning}
              </p>
            ) : null}
            <Button
              type="button"
              variant="destructive"
              size="sm"
              disabled={!rotateConfirm || rotate.isPending}
              onClick={() => rotate.mutate()}
            >
              {rotate.isPending ? "Rotating…" : "Rotate key"}
            </Button>
          </div>
        ) : null}
      </DialogContent>
    </Dialog>
  );
}

// ---------- One-time key reveal ----------

/**
 * Shown after a successful encrypted-store create or rotate-key call.
 * The key value lives only in this component's state — it is dropped
 * the moment the operator confirms the acknowledgement checkbox and
 * closes the dialog. Never written to localStorage, sessionStorage,
 * cookies, or the URL.
 */
function KeyRevealDialog({
  title,
  description,
  keyHex,
  onClose,
}: {
  title: string;
  description: ReactNode;
  keyHex: string;
  onClose: () => void;
}) {
  const [ack, setAck] = useState(false);
  const [copyStatus, setCopyStatus] = useState<"idle" | "copied" | "failed">(
    "idle",
  );

  async function onCopy() {
    try {
      if (typeof navigator !== "undefined" && navigator.clipboard) {
        await navigator.clipboard.writeText(keyHex);
        setCopyStatus("copied");
      } else {
        setCopyStatus("failed");
      }
    } catch {
      setCopyStatus("failed");
    }
  }

  return (
    <Dialog
      open
      onOpenChange={(o) => {
        // Only allow closing via the explicit confirm button after the
        // ack box is ticked. Ignore escape / outside clicks while the
        // key is visible so a stray click can't bury it.
        if (!o && ack) onClose();
      }}
    >
      <DialogContent
        data-testid="key-reveal-dialog"
        onEscapeKeyDown={(e) => {
          if (!ack) e.preventDefault();
        }}
        onPointerDownOutside={(e) => {
          if (!ack) e.preventDefault();
        }}
        onInteractOutside={(e) => {
          if (!ack) e.preventDefault();
        }}
      >
        <DialogHeader>
          <DialogTitle>{title}</DialogTitle>
          <DialogDescription>{description}</DialogDescription>
        </DialogHeader>
        <div className="space-y-3">
          <Label htmlFor="key-reveal-value" className="text-xs">
            Master key (hex)
          </Label>
          <pre
            id="key-reveal-value"
            data-testid="key-reveal-value"
            className="overflow-x-auto rounded-md border border-(--color-line) bg-(--color-surface-1) p-3 font-mono text-xs text-(--color-fg)"
          >
            {keyHex}
          </pre>
          <div className="flex items-center gap-2">
            <Button type="button" size="sm" variant="ghost" onClick={onCopy}>
              Copy to clipboard
            </Button>
            {copyStatus === "copied" ? (
              <span className="text-xs text-emerald-300">Copied.</span>
            ) : copyStatus === "failed" ? (
              <span className="text-xs text-rose-300">
                Could not access clipboard — select the value above and
                copy by hand.
              </span>
            ) : null}
          </div>
          <label className="flex items-start gap-2 text-xs text-(--color-muted)">
            <input
              type="checkbox"
              data-testid="key-reveal-ack"
              checked={ack}
              onChange={(e) => setAck(e.target.checked)}
            />
            <span>I have saved this key securely.</span>
          </label>
        </div>
        <DialogFooter>
          <Button
            type="button"
            data-testid="key-reveal-confirm"
            disabled={!ack}
            onClick={onClose}
          >
            Done
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}

// ---------- Delete ----------

function DeleteStoreDialog({
  store,
  onClose,
}: {
  store: AdminStoreDetailResponse;
  onClose: () => void;
}) {
  const qc = useQueryClient();
  const [force, setForce] = useState(false);
  const [submitError, setSubmitError] = useState<string | null>(null);
  const [cleanupHint, setCleanupHint] = useState<string | null>(null);

  const mut = useMutation({
    mutationFn: (opts: { force: boolean }) => deleteStore(store.id, opts),
    onSuccess: async (res) => {
      await qc.invalidateQueries({ queryKey: ["admin", "stores"] });
      if (res.cleanup_hint) {
        // Surface the hint and require an explicit dismissal so the
        // operator notices the data dir still has secrets on disk.
        setCleanupHint(res.cleanup_hint);
      } else {
        onClose();
      }
    },
    onError: (err) => setSubmitError(describeApiError(err)),
  });

  const requiresForce = !store.actions.can_delete_now && store.actions.can_force_delete;
  const blockedByMapping =
    !store.actions.can_delete_now &&
    !store.actions.can_force_delete &&
    store.usage.mapped_client_instance_count > 0;
  const canDelete =
    !mut.isPending &&
    (store.actions.can_delete_now || (store.actions.can_force_delete && force));
  if (cleanupHint) {
    return (
      <Dialog open onOpenChange={(o) => !o && onClose()}>
        <DialogContent>
          <DialogHeader>
            <DialogTitle>Store deleted</DialogTitle>
            <DialogDescription>
              The YAML definition for <strong>{store.name}</strong> was
              removed. The data directory still contains secrets on
              disk — follow up manually.
            </DialogDescription>
          </DialogHeader>
          <pre className="overflow-x-auto rounded-md border border-(--color-line) bg-(--color-surface-1) p-3 text-xs text-(--color-muted)">
            {cleanupHint}
          </pre>
          <DialogFooter>
            <Button onClick={onClose}>Close</Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    );
  }
  return (
    <Dialog open onOpenChange={(o) => !o && onClose()}>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>Delete store: {store.name}</DialogTitle>
          <DialogDescription>
            This removes the store definition and stops the backend
            from serving it. Secret material on disk is not touched.
          </DialogDescription>
        </DialogHeader>
        <div className="space-y-3">
          {blockedByMapping ? (
            <p className="rounded-md border border-amber-500/30 bg-amber-500/10 px-3 py-2 text-xs text-amber-200">
              This store is still mapped to {store.usage.mapped_client_instance_count} client
              {store.usage.mapped_client_instance_count === 1 ? "" : "s"}. Remap or disable those
              instances before deleting the store.
            </p>
          ) : null}
          {requiresForce ? (
            <p className="rounded-md border border-amber-500/30 bg-amber-500/10 px-3 py-2 text-xs text-amber-200">
              This store currently contains secrets. Deleting it requires an explicit force-delete
              confirmation; the backend will remove only the store record and leave the on-disk data
              directory in place for manual follow-up.
            </p>
          ) : null}
          {store.actions.can_force_delete ? (
            <label className="flex items-center gap-2 text-xs text-(--color-muted)">
              <input
                type="checkbox"
                checked={force}
                onChange={(e) => setForce(e.target.checked)}
              />
              Force delete even if the store contains secrets
            </label>
          ) : null}
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
            onClick={() => mut.mutate({ force: requiresForce || force })}
          >
            {mut.isPending ? "Deleting…" : requiresForce ? "Force delete" : "Delete"}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}

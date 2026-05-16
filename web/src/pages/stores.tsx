import { useEffect, useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useForm } from "react-hook-form";
import { zodResolver } from "@hookform/resolvers/zod";
import { z } from "zod";
import { ShieldCheck, ShieldOff } from "lucide-react";

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
  type AdminStoreCreateRequest,
  type AdminStoreDetailResponse,
  type AdminStoresListResponse,
  deleteStore,
  fetchStores,
  postStoreCreate,
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

/** True when a store's row should show its mutation controls disabled. */
export function isStoreReadOnly(
  source: AdminStoreDetailResponse["source"],
): boolean {
  return source === "declarative";
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
            Declarative stores are owned by the Nix configuration and
            cannot be edited from the UI. Dynamic stores are managed
            here.
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
          onEdit={(item) =>
            setEditing(
              // The list response is a subset of the detail shape;
              // missing fields are filled in on edit-load.
              {
                name: item.name,
                source: item.source,
                backend: item.backend,
                description: item.description,
                created_at: item.created_at,
                dir: "",
                source_path: "",
              } satisfies AdminStoreDetailResponse,
            )
          }
          onDelete={(item) =>
            setDeleting({
              name: item.name,
              source: item.source,
              backend: item.backend,
              description: item.description,
              created_at: item.created_at,
              dir: "",
              source_path: "",
            } satisfies AdminStoreDetailResponse)
          }
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
            <th className="px-4 py-2">Source</th>
            <th className="px-4 py-2">Backend</th>
            <th className="px-4 py-2">Description</th>
            <th className="px-4 py-2 text-right">Actions</th>
          </tr>
        </thead>
        <tbody>
          {items.map((item) => {
            const readOnly = isStoreReadOnly(item.source);
            const disabledTitle = readOnly
              ? "Declarative store — edit the Nix module that owns it."
              : undefined;
            return (
              <tr
                key={item.name}
                className="border-t border-(--color-line) align-middle"
              >
                <td className="px-4 py-3 font-medium text-(--color-fg)">
                  {item.name}
                </td>
                <td className="px-4 py-3">
                  <SourceBadge source={item.source} />
                </td>
                <td className="px-4 py-3 text-(--color-muted)">
                  {item.backend}
                </td>
                <td className="px-4 py-3 text-(--color-muted)">
                  {item.description ?? "—"}
                </td>
                <td className="px-4 py-3 text-right">
                  <div className="inline-flex gap-2">
                    <Button
                      variant="ghost"
                      size="sm"
                      disabled={!isAdmin || readOnly}
                      title={disabledTitle}
                      onClick={() => onEdit(item)}
                    >
                      Edit
                    </Button>
                    <Button
                      variant="destructive"
                      size="sm"
                      disabled={!isAdmin || readOnly}
                      title={disabledTitle}
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

function SourceBadge({
  source,
}: {
  source: AdminStoresListResponse["items"][number]["source"];
}) {
  const isDeclarative = source === "declarative";
  return (
    <span
      className={
        "inline-flex items-center gap-1.5 rounded-md border px-2 py-0.5 text-xs " +
        (isDeclarative
          ? "border-amber-500/30 bg-amber-500/10 text-amber-300"
          : "border-emerald-500/30 bg-emerald-500/10 text-emerald-300")
      }
    >
      {isDeclarative ? (
        <ShieldOff className="h-3 w-3" aria-hidden />
      ) : (
        <ShieldCheck className="h-3 w-3" aria-hidden />
      )}
      {source}
    </span>
  );
}

// ---------- Create ----------

const createSchema = z
  .object({
    name: z
      .string()
      .min(1, "Store name is required.")
      .regex(
        /^[a-z0-9][a-z0-9_-]*$/,
        "Use lowercase letters, digits, '-' or '_'; must start with a letter or digit.",
      ),
    backend: z.enum(["plain", "encrypted"]),
    dir: z.string().min(1, "Data directory is required."),
    key_file: z.string().optional(),
    description: z.string().optional(),
  })
  .refine(
    (v) => v.backend !== "encrypted" || (v.key_file && v.key_file.length > 0),
    {
      path: ["key_file"],
      message: "Key file is required for encrypted stores.",
    },
  );
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
  const form = useForm<CreateValues>({
    resolver: zodResolver(createSchema),
    defaultValues: {
      name: "",
      backend: "plain",
      dir: "",
      key_file: "",
      description: "",
    },
  });
  const backend = form.watch("backend");

  const create = useMutation({
    mutationFn: (body: AdminStoreCreateRequest) => postStoreCreate(body),
    onSuccess: async () => {
      await qc.invalidateQueries({ queryKey: ["admin", "stores"] });
      onClose();
    },
    onError: (err) => setSubmitError(describeApiError(err)),
  });

  function onSubmit(values: CreateValues) {
    setSubmitError(null);
    const body: AdminStoreCreateRequest = {
      name: values.name,
      backend: values.backend,
      dir: values.dir,
    };
    if (values.backend === "encrypted") {
      body.key_file = values.key_file;
    }
    if (values.description) body.description = values.description;
    create.mutate(body);
  }

  return (
    <Dialog open={open} onOpenChange={(o) => !o && onClose()}>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>Create store</DialogTitle>
          <DialogDescription>
            The data directory must be under one of the configured
            <code className="mx-1 rounded bg-(--color-surface-1) px-1 py-0.5 text-xs">stores_data_roots</code>
            prefixes on the backend.
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
            <FormField
              control={form.control}
              name="dir"
              render={({ field }) => (
                <FormItem>
                  <FormLabel>Data directory (absolute path)</FormLabel>
                  <FormControl>
                    <Input
                      placeholder="/var/lib/underwrap-backend/data/work"
                      autoCapitalize="off"
                      autoCorrect="off"
                      spellCheck={false}
                      {...field}
                    />
                  </FormControl>
                  <FormMessage />
                </FormItem>
              )}
            />
            {backend === "encrypted" ? (
              <FormField
                control={form.control}
                name="key_file"
                render={({ field }) => (
                  <FormItem>
                    <FormLabel>Key file path</FormLabel>
                    <FormControl>
                      <Input
                        placeholder="/run/keys/underwrap-work.key"
                        autoCapitalize="off"
                        autoCorrect="off"
                        spellCheck={false}
                        {...field}
                      />
                    </FormControl>
                    <FormDescription>
                      32-byte master key file readable only by the
                      <code className="mx-1 rounded bg-(--color-surface-1) px-1 py-0.5 text-xs">underwrap-backend</code>
                      user.
                    </FormDescription>
                    <FormMessage />
                  </FormItem>
                )}
              />
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
  const form = useForm<EditValues>({
    resolver: zodResolver(editSchema),
    defaultValues: { description: store.description ?? "" },
  });
  const mut = useMutation({
    mutationFn: (description: string) =>
      putStoreUpdate(store.name, { description }),
    onSuccess: async () => {
      await qc.invalidateQueries({ queryKey: ["admin", "stores"] });
      onClose();
    },
    onError: (err) => setSubmitError(describeApiError(err)),
  });

  return (
    <Dialog open onOpenChange={(o) => !o && onClose()}>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>Edit store: {store.name}</DialogTitle>
          <DialogDescription>
            Backend type and data directory are immutable. Only the
            description can be edited here. Encrypted-store key rotation
            lands in a follow-up ticket.
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
  const [confirmText, setConfirmText] = useState("");
  const [force, setForce] = useState(false);
  const [submitError, setSubmitError] = useState<string | null>(null);
  const [cleanupHint, setCleanupHint] = useState<string | null>(null);

  const mut = useMutation({
    mutationFn: (opts: { force: boolean }) => deleteStore(store.name, opts),
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

  const canDelete = confirmText === store.name && !mut.isPending;
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
            Type the store name to confirm.
          </DialogDescription>
        </DialogHeader>
        <div className="space-y-3">
          <div className="space-y-1.5">
            <Label htmlFor="confirm-name">
              Type <code className="rounded bg-(--color-surface-1) px-1 py-0.5">{store.name}</code> to confirm
            </Label>
            <Input
              id="confirm-name"
              value={confirmText}
              onChange={(e) => setConfirmText(e.target.value)}
              autoFocus
              autoComplete="off"
            />
          </div>
          <label className="flex items-center gap-2 text-xs text-(--color-muted)">
            <input
              type="checkbox"
              checked={force}
              onChange={(e) => setForce(e.target.checked)}
            />
            Force delete even if the store contains secrets
          </label>
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
            onClick={() => mut.mutate({ force })}
          >
            {mut.isPending ? "Deleting…" : "Delete"}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}

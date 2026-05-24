import { useMemo, useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useForm } from "react-hook-form";
import { zodResolver } from "@hookform/resolvers/zod";
import { z } from "zod";
import { ChevronRight, Eye, Plus, Trash2 } from "lucide-react";

import { Alert } from "@/components/ui/alert";
import { Button } from "@/components/ui/button";
import { ConfirmDialog } from "@/components/confirm-dialog";
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
import { Select } from "@/components/ui/select";
import {
  type AdminLibsecretCollectionView,
  type AdminLibsecretItemView,
  type AdminLibsecretTreeResponse,
  deleteLibsecretAlias,
  deleteLibsecretCollection,
  deleteLibsecretItem,
  fetchLibsecretItemSecret,
  fetchLibsecretTree,
  postLibsecretCollection,
  postLibsecretItem,
  putLibsecretAlias,
} from "@/api/client";
import { describeApiError } from "@/lib/error-message";

// Operator-supplied ids are validated by the backend with
// `[A-Za-z0-9._-]+`, max 128, no leading `_`. Mirror the rule in the
// client for inline form feedback; failure messages from the backend
// remain authoritative.
const ID_PATTERN = /^(?!_)[A-Za-z0-9._-]+$/;

function libsecretTreeKey(storeName: string, resourceId: string) {
  return ["admin", "libsecret", storeName, resourceId] as const;
}

export function LibsecretTree({
  storeName,
  resourceId,
}: {
  storeName: string;
  resourceId: string;
}) {
  const tree = useQuery<AdminLibsecretTreeResponse>({
    queryKey: libsecretTreeKey(storeName, resourceId),
    queryFn: () => fetchLibsecretTree(storeName, resourceId),
  });

  const [creatingCollection, setCreatingCollection] = useState(false);
  const [creatingItem, setCreatingItem] = useState<{ collectionId: string } | null>(
    null,
  );
  const [revealing, setRevealing] = useState<
    { collectionId: string; itemId: string } | null
  >(null);
  const [deletingCollection, setDeletingCollection] = useState<string | null>(null);
  const [deletingItem, setDeletingItem] = useState<
    { collectionId: string; itemId: string } | null
  >(null);
  const [creatingAlias, setCreatingAlias] = useState(false);
  const [deletingAlias, setDeletingAlias] = useState<string | null>(null);

  if (tree.isLoading) {
    return <p className="pl-2 text-xs text-(--color-muted)">Loading libsecret…</p>;
  }
  if (tree.error) {
    return <Alert>{describeApiError(tree.error)}</Alert>;
  }

  const data = tree.data;
  if (!data) {
    return null;
  }

  return (
    <section
      data-testid="libsecret-tree"
      className="space-y-3"
      aria-label="libsecret"
    >
      <div className="flex items-center gap-2">
        <h3 className="font-mono text-xs uppercase tracking-wide text-(--color-muted)">
          libsecret
        </h3>
        <Button
          type="button"
          variant="ghost"
          size="sm"
          aria-label="Add libsecret collection"
          data-testid="libsecret-add-collection"
          onClick={() => setCreatingCollection(true)}
          className="h-6 px-1.5"
        >
          <Plus className="h-3.5 w-3.5" aria-hidden />
          Collection
        </Button>
      </div>

      {data.collections.length === 0 && Object.keys(data.aliases).length === 0 ? (
        <p className="pl-2 text-xs text-(--color-muted)/70">(none)</p>
      ) : null}

      {data.collections.map((collection) => (
        <CollectionBlock
          key={collection.id}
          collection={collection}
          onAddItem={() =>
            setCreatingItem({ collectionId: collection.id })
          }
          onDelete={() => setDeletingCollection(collection.id)}
          onRevealItem={(itemId) =>
            setRevealing({ collectionId: collection.id, itemId })
          }
          onDeleteItem={(itemId) =>
            setDeletingItem({ collectionId: collection.id, itemId })
          }
        />
      ))}

      <AliasesPanel
        aliases={data.aliases}
        collections={data.collections}
        onCreate={() => setCreatingAlias(true)}
        onDelete={(name) => setDeletingAlias(name)}
      />

      {creatingCollection ? (
        <CreateCollectionDialog
          storeName={storeName}
          resourceId={resourceId}
          existingIds={data.collections.map((c) => c.id)}
          onClose={() => setCreatingCollection(false)}
        />
      ) : null}
      {creatingItem ? (
        <CreateItemDialog
          storeName={storeName}
          resourceId={resourceId}
          collectionId={creatingItem.collectionId}
          existingIds={
            data.collections.find((c) => c.id === creatingItem.collectionId)
              ?.items.map((i) => i.id) ?? []
          }
          onClose={() => setCreatingItem(null)}
        />
      ) : null}
      {revealing ? (
        <RevealItemSecretDialog
          storeName={storeName}
          resourceId={resourceId}
          collectionId={revealing.collectionId}
          itemId={revealing.itemId}
          onClose={() => setRevealing(null)}
        />
      ) : null}
      {deletingCollection ? (
        <DeleteCollectionDialog
          storeName={storeName}
          resourceId={resourceId}
          collectionId={deletingCollection}
          onClose={() => setDeletingCollection(null)}
        />
      ) : null}
      {deletingItem ? (
        <DeleteItemDialog
          storeName={storeName}
          resourceId={resourceId}
          collectionId={deletingItem.collectionId}
          itemId={deletingItem.itemId}
          onClose={() => setDeletingItem(null)}
        />
      ) : null}
      {creatingAlias ? (
        <SetAliasDialog
          storeName={storeName}
          resourceId={resourceId}
          collections={data.collections}
          existingAliases={Object.keys(data.aliases)}
          onClose={() => setCreatingAlias(false)}
        />
      ) : null}
      {deletingAlias ? (
        <DeleteAliasDialog
          storeName={storeName}
          resourceId={resourceId}
          name={deletingAlias}
          onClose={() => setDeletingAlias(null)}
        />
      ) : null}
    </section>
  );
}

function CollectionBlock({
  collection,
  onAddItem,
  onDelete,
  onRevealItem,
  onDeleteItem,
}: {
  collection: AdminLibsecretCollectionView;
  onAddItem: () => void;
  onDelete: () => void;
  onRevealItem: (itemId: string) => void;
  onDeleteItem: (itemId: string) => void;
}) {
  return (
    <details
      data-testid={`libsecret-collection-${collection.id}`}
      className="group rounded-md border border-(--color-line)/60 bg-(--color-surface-0)/30 open:bg-(--color-surface-0)/60"
    >
      <summary className="flex cursor-pointer list-none items-center gap-2 px-3 py-2 text-sm">
        <ChevronRight
          className="h-4 w-4 transition-transform group-open:rotate-90"
          aria-hidden
        />
        <span className="font-mono text-(--color-fg)">{collection.label}</span>
        <span
          className="font-mono text-[0.65rem] text-(--color-muted)/70"
          title={`id: ${collection.id}`}
        >
          {collection.id}
        </span>
        <span className="ml-auto text-xs text-(--color-muted)">
          {collection.items.length === 0
            ? "empty"
            : `${collection.items.length} item${
                collection.items.length === 1 ? "" : "s"
              }`}
        </span>
        <Button
          type="button"
          variant="ghost"
          size="sm"
          aria-label={`Add item to ${collection.label}`}
          data-testid={`libsecret-add-item-${collection.id}`}
          onClick={(e) => {
            e.preventDefault();
            e.stopPropagation();
            onAddItem();
          }}
          className="h-7 px-2"
        >
          <Plus className="h-3.5 w-3.5" aria-hidden />
        </Button>
        <Button
          type="button"
          variant="ghost"
          size="sm"
          aria-label={`Delete collection ${collection.label}`}
          data-testid={`libsecret-delete-collection-${collection.id}`}
          onClick={(e) => {
            e.preventDefault();
            e.stopPropagation();
            onDelete();
          }}
          className="h-7 px-2"
        >
          <Trash2 className="h-3.5 w-3.5" aria-hidden />
        </Button>
      </summary>
      <div className="space-y-1 border-t border-(--color-line)/40 px-3 py-2">
        {collection.items.length === 0 ? (
          <p className="text-xs text-(--color-muted)/70">(no items)</p>
        ) : (
          <ul className="space-y-1">
            {collection.items.map((item) => (
              <li
                key={item.id}
                data-testid={`libsecret-item-${collection.id}-${item.id}`}
                className="flex flex-col gap-1 rounded-md px-2 py-1.5 hover:bg-(--color-surface-1)/50 sm:flex-row sm:items-center sm:gap-2"
              >
                <span className="font-mono text-xs text-(--color-fg)">
                  {item.label}
                </span>
                <span
                  className="font-mono text-[0.65rem] text-(--color-muted)/70"
                  title={`content-type: ${item.content_type}`}
                >
                  {item.id} · {item.content_type}
                </span>
                {Object.keys(item.attributes).length > 0 ? (
                  <ItemAttributes attributes={item.attributes} />
                ) : null}
                <div className="ml-auto inline-flex gap-2">
                  <Button
                    variant="ghost"
                    size="sm"
                    onClick={() => onRevealItem(item.id)}
                  >
                    <Eye className="mr-1.5 h-4 w-4" aria-hidden />
                    Reveal
                  </Button>
                  <Button
                    variant="destructive"
                    size="sm"
                    onClick={() => onDeleteItem(item.id)}
                  >
                    <Trash2 className="mr-1.5 h-4 w-4" aria-hidden />
                    Delete
                  </Button>
                </div>
              </li>
            ))}
          </ul>
        )}
      </div>
    </details>
  );
}

function ItemAttributes({ attributes }: { attributes: AdminLibsecretItemView["attributes"] }) {
  // Render attributes inline, compact. The list is normally short
  // (xdg:schema + a couple of lookup keys), so a single line is fine
  // for the typical case.
  const entries = Object.entries(attributes);
  return (
    <span
      className="font-mono text-[0.65rem] text-(--color-muted)/80"
      data-testid="libsecret-item-attributes"
    >
      {entries.map(([k, v], idx) => (
        <span key={k}>
          {idx > 0 ? ", " : null}
          {k}={v}
        </span>
      ))}
    </span>
  );
}

function AliasesPanel({
  aliases,
  collections,
  onCreate,
  onDelete,
}: {
  aliases: AdminLibsecretTreeResponse["aliases"];
  collections: AdminLibsecretCollectionView[];
  onCreate: () => void;
  onDelete: (name: string) => void;
}) {
  const entries = Object.entries(aliases);
  const danglingTargets = useMemo(() => {
    const known = new Set(collections.map((c) => c.id));
    return new Set(
      entries.filter(([, target]) => !known.has(target)).map(([name]) => name),
    );
  }, [entries, collections]);

  return (
    <section
      data-testid="libsecret-aliases"
      className="rounded-md border border-(--color-line)/60 bg-(--color-surface-0)/30 px-3 py-2"
      aria-label="libsecret aliases"
    >
      <div className="flex items-center gap-2">
        <h4 className="font-mono text-xs uppercase tracking-wide text-(--color-muted)">
          aliases
        </h4>
        <Button
          type="button"
          variant="ghost"
          size="sm"
          aria-label="Set alias"
          data-testid="libsecret-set-alias"
          onClick={onCreate}
          className="h-6 px-1.5"
          disabled={collections.length === 0}
          title={
            collections.length === 0
              ? "Add a collection before setting an alias"
              : undefined
          }
        >
          <Plus className="h-3.5 w-3.5" aria-hidden />
        </Button>
      </div>
      {entries.length === 0 ? (
        <p className="pl-1 pt-1 text-xs text-(--color-muted)/70">(none)</p>
      ) : (
        <ul className="space-y-1 pt-1">
          {entries.map(([name, target]) => (
            <li
              key={name}
              data-testid={`libsecret-alias-${name}`}
              className="flex items-center gap-2 rounded-md px-2 py-1.5 text-xs hover:bg-(--color-surface-1)/50"
            >
              <span className="font-mono text-(--color-fg)">{name}</span>
              <span className="text-(--color-muted)/70">→</span>
              <span className="font-mono text-(--color-fg)">{target}</span>
              {danglingTargets.has(name) ? (
                <span
                  className="ml-1 rounded bg-(--color-warning)/20 px-1.5 py-0.5 text-[0.65rem] text-(--color-warning)"
                  title="The target collection does not exist."
                >
                  dangling
                </span>
              ) : null}
              <div className="ml-auto">
                <Button
                  variant="destructive"
                  size="sm"
                  onClick={() => onDelete(name)}
                >
                  <Trash2 className="mr-1.5 h-4 w-4" aria-hidden />
                  Delete
                </Button>
              </div>
            </li>
          ))}
        </ul>
      )}
    </section>
  );
}

// ---- Dialogs --------------------------------------------------------------

const collectionSchema = z.object({
  id: z
    .string()
    .optional()
    .refine(
      (s) => !s || ID_PATTERN.test(s),
      "Id must match [A-Za-z0-9._-]+ and not start with '_'.",
    ),
  label: z.string().min(1, "Label is required."),
});

function CreateCollectionDialog({
  storeName,
  resourceId,
  existingIds,
  onClose,
}: {
  storeName: string;
  resourceId: string;
  existingIds: string[];
  onClose: () => void;
}) {
  const qc = useQueryClient();
  const [error, setError] = useState<string | null>(null);
  const form = useForm<z.infer<typeof collectionSchema>>({
    resolver: zodResolver(collectionSchema),
    defaultValues: { id: "", label: "" },
  });
  const mut = useMutation({
    mutationFn: (v: z.infer<typeof collectionSchema>) =>
      postLibsecretCollection(storeName, resourceId, {
        id: v.id || undefined,
        label: v.label,
      }),
  });
  async function onSubmit(v: z.infer<typeof collectionSchema>) {
    setError(null);
    if (v.id && existingIds.includes(v.id)) {
      form.setError("id", { message: "Id already exists." });
      return;
    }
    try {
      await mut.mutateAsync(v);
      await qc.invalidateQueries({
        queryKey: libsecretTreeKey(storeName, resourceId),
      });
      onClose();
    } catch (e) {
      setError(describeApiError(e));
    }
  }
  return (
    <Dialog open onOpenChange={(o) => !o && onClose()}>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>Create libsecret collection</DialogTitle>
          <DialogDescription>
            A collection holds Secret Service items inside resource{" "}
            <code>{resourceId}</code>.
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
              name="id"
              render={({ field }) => (
                <FormItem>
                  <FormLabel>Id (optional)</FormLabel>
                  <FormControl>
                    <Input
                      autoFocus
                      autoCapitalize="off"
                      autoCorrect="off"
                      spellCheck={false}
                      placeholder="auto-generated if blank, e.g. default"
                      {...field}
                    />
                  </FormControl>
                  <FormDescription>
                    Stable identifier. Libsecret clients reach this
                    collection via <code>ReadAlias</code> or by id.
                  </FormDescription>
                  <FormMessage />
                </FormItem>
              )}
            />
            <FormField
              control={form.control}
              name="label"
              render={({ field }) => (
                <FormItem>
                  <FormLabel>Label</FormLabel>
                  <FormControl>
                    <Input
                      autoCapitalize="off"
                      autoCorrect="off"
                      spellCheck={false}
                      placeholder="e.g. Default"
                      {...field}
                    />
                  </FormControl>
                  <FormMessage />
                </FormItem>
              )}
            />
            {error ? <Alert>{error}</Alert> : null}
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

const itemSchema = z.object({
  id: z
    .string()
    .optional()
    .refine(
      (s) => !s || ID_PATTERN.test(s),
      "Id must match [A-Za-z0-9._-]+ and not start with '_'.",
    ),
  label: z.string().min(1, "Label is required."),
  content_type: z.string().min(1, "Content type is required."),
  attributes: z.string(),
  secret: z.string().min(1, "Secret material is required."),
});

function parseAttributes(input: string): Record<string, string> {
  // `key=value` per line; blank lines and lines without `=` ignored.
  // Keys / values trimmed. Duplicate keys: last wins.
  const out: Record<string, string> = {};
  for (const line of input.split("\n")) {
    const idx = line.indexOf("=");
    if (idx < 0) continue;
    const key = line.slice(0, idx).trim();
    const value = line.slice(idx + 1).trim();
    if (!key) continue;
    out[key] = value;
  }
  return out;
}

function CreateItemDialog({
  storeName,
  resourceId,
  collectionId,
  existingIds,
  onClose,
}: {
  storeName: string;
  resourceId: string;
  collectionId: string;
  existingIds: string[];
  onClose: () => void;
}) {
  const qc = useQueryClient();
  const [error, setError] = useState<string | null>(null);
  const form = useForm<z.infer<typeof itemSchema>>({
    resolver: zodResolver(itemSchema),
    defaultValues: {
      id: "",
      label: "",
      content_type: "text/plain",
      attributes: "",
      secret: "",
    },
  });
  const mut = useMutation({
    mutationFn: (v: z.infer<typeof itemSchema>) =>
      postLibsecretItem(storeName, resourceId, collectionId, {
        id: v.id || undefined,
        label: v.label,
        content_type: v.content_type,
        attributes: parseAttributes(v.attributes),
        secret_b64: btoa(v.secret),
      }),
  });
  async function onSubmit(v: z.infer<typeof itemSchema>) {
    setError(null);
    if (v.id && existingIds.includes(v.id)) {
      form.setError("id", { message: "Id already exists in this collection." });
      return;
    }
    try {
      await mut.mutateAsync(v);
      await qc.invalidateQueries({
        queryKey: libsecretTreeKey(storeName, resourceId),
      });
      onClose();
    } catch (e) {
      setError(describeApiError(e));
    }
  }
  return (
    <Dialog open onOpenChange={(o) => !o && onClose()}>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>Create libsecret item</DialogTitle>
          <DialogDescription>
            In collection <code>{collectionId}</code> of resource{" "}
            <code>{resourceId}</code>. Secret is sent over TLS,
            base64-encoded in transit, and never echoed back.
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
              name="id"
              render={({ field }) => (
                <FormItem>
                  <FormLabel>Id (optional)</FormLabel>
                  <FormControl>
                    <Input
                      autoFocus
                      autoCapitalize="off"
                      autoCorrect="off"
                      spellCheck={false}
                      placeholder="auto-generated if blank, e.g. item_1"
                      {...field}
                    />
                  </FormControl>
                  <FormMessage />
                </FormItem>
              )}
            />
            <FormField
              control={form.control}
              name="label"
              render={({ field }) => (
                <FormItem>
                  <FormLabel>Label</FormLabel>
                  <FormControl>
                    <Input
                      autoCapitalize="off"
                      autoCorrect="off"
                      spellCheck={false}
                      placeholder="e.g. GitHub Token"
                      {...field}
                    />
                  </FormControl>
                  <FormMessage />
                </FormItem>
              )}
            />
            <FormField
              control={form.control}
              name="content_type"
              render={({ field }) => (
                <FormItem>
                  <FormLabel>Content type</FormLabel>
                  <FormControl>
                    <Input
                      autoCapitalize="off"
                      autoCorrect="off"
                      spellCheck={false}
                      placeholder="text/plain"
                      {...field}
                    />
                  </FormControl>
                  <FormDescription>
                    The MIME-style type libsecret carries in the Secret
                    struct. Most apps use <code>text/plain</code>.
                  </FormDescription>
                  <FormMessage />
                </FormItem>
              )}
            />
            <FormField
              control={form.control}
              name="attributes"
              render={({ field }) => (
                <FormItem>
                  <FormLabel>Attributes (optional)</FormLabel>
                  <FormControl>
                    <textarea
                      data-testid="libsecret-item-attributes-input"
                      rows={3}
                      className="w-full rounded-md border border-(--color-line) bg-(--color-surface-0) p-2 font-mono text-xs text-(--color-fg)"
                      placeholder={"xdg:schema=org.freedesktop.Secret.Generic\nservice=github.com"}
                      {...field}
                    />
                  </FormControl>
                  <FormDescription>
                    One <code>key=value</code> per line. libsecret looks
                    these up case-sensitively.
                  </FormDescription>
                  <FormMessage />
                </FormItem>
              )}
            />
            <FormField
              control={form.control}
              name="secret"
              render={({ field }) => (
                <FormItem>
                  <FormLabel>Secret material</FormLabel>
                  <FormControl>
                    <textarea
                      data-testid="libsecret-item-secret-input"
                      rows={4}
                      className="w-full rounded-md border border-(--color-line) bg-(--color-surface-0) p-2 font-mono text-xs text-(--color-fg)"
                      {...field}
                    />
                  </FormControl>
                  <FormMessage />
                </FormItem>
              )}
            />
            {error ? <Alert>{error}</Alert> : null}
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

function RevealItemSecretDialog({
  storeName,
  resourceId,
  collectionId,
  itemId,
  onClose,
}: {
  storeName: string;
  resourceId: string;
  collectionId: string;
  itemId: string;
  onClose: () => void;
}) {
  const [confirmed, setConfirmed] = useState(false);
  const [copied, setCopied] = useState(false);
  const read = useQuery({
    queryKey: [
      "admin",
      "libsecret",
      "item-secret",
      storeName,
      resourceId,
      collectionId,
      itemId,
    ],
    queryFn: () =>
      fetchLibsecretItemSecret(storeName, resourceId, collectionId, itemId),
    enabled: confirmed,
    staleTime: 0,
    gcTime: 0,
    refetchOnMount: "always",
  });
  const decoded = useMemo(() => {
    if (!read.data?.secret_b64) return null;
    try {
      return atob(read.data.secret_b64);
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
      // ignore
    }
  }
  return (
    <Dialog open onOpenChange={(o) => !o && onClose()}>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>Reveal libsecret item secret</DialogTitle>
          <DialogDescription>
            <code>
              {collectionId} · {itemId}
            </code>{" "}
            in resource <code>{resourceId}</code>.
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
          <Alert>{describeApiError(read.error)}</Alert>
        ) : decoded == null ? (
          <Alert variant="warning">
            The secret material could not be decoded as UTF-8 text.
          </Alert>
        ) : (
          <div className="space-y-2">
            <Label htmlFor="libsecret-reveal-value">Value</Label>
            <textarea
              id="libsecret-reveal-value"
              data-testid="libsecret-reveal-value"
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

function DeleteCollectionDialog({
  storeName,
  resourceId,
  collectionId,
  onClose,
}: {
  storeName: string;
  resourceId: string;
  collectionId: string;
  onClose: () => void;
}) {
  const qc = useQueryClient();
  const mut = useMutation({
    mutationFn: () =>
      deleteLibsecretCollection(storeName, resourceId, collectionId),
    onSuccess: async () => {
      await qc.invalidateQueries({
        queryKey: libsecretTreeKey(storeName, resourceId),
      });
      onClose();
    },
  });
  return (
    <ConfirmDialog
      title="Delete collection"
      description={
        <>
          Removes collection <code>{collectionId}</code> and all its
          items. Any aliases pointing at this collection are scrubbed
          atomically. This cannot be undone.
        </>
      }
      destructive
      confirmLabel="Delete"
      pendingLabel="Deleting…"
      onConfirm={() => mut.mutateAsync()}
      onClose={onClose}
    />
  );
}

function DeleteItemDialog({
  storeName,
  resourceId,
  collectionId,
  itemId,
  onClose,
}: {
  storeName: string;
  resourceId: string;
  collectionId: string;
  itemId: string;
  onClose: () => void;
}) {
  const qc = useQueryClient();
  const mut = useMutation({
    mutationFn: () =>
      deleteLibsecretItem(storeName, resourceId, collectionId, itemId),
    onSuccess: async () => {
      await qc.invalidateQueries({
        queryKey: libsecretTreeKey(storeName, resourceId),
      });
      onClose();
    },
  });
  return (
    <ConfirmDialog
      title="Delete item"
      description={
        <>
          Removes <code>{collectionId} · {itemId}</code> from resource{" "}
          <code>{resourceId}</code>. This cannot be undone.
        </>
      }
      destructive
      confirmLabel="Delete"
      pendingLabel="Deleting…"
      onConfirm={() => mut.mutateAsync()}
      onClose={onClose}
    />
  );
}

const aliasSchema = z.object({
  name: z
    .string()
    .min(1, "Alias name is required.")
    .refine(
      (s) => ID_PATTERN.test(s),
      "Name must match [A-Za-z0-9._-]+ and not start with '_'.",
    ),
  collection_id: z.string().min(1, "Pick a collection."),
});

function SetAliasDialog({
  storeName,
  resourceId,
  collections,
  existingAliases,
  onClose,
}: {
  storeName: string;
  resourceId: string;
  collections: AdminLibsecretCollectionView[];
  existingAliases: string[];
  onClose: () => void;
}) {
  const qc = useQueryClient();
  const [error, setError] = useState<string | null>(null);
  const form = useForm<z.infer<typeof aliasSchema>>({
    resolver: zodResolver(aliasSchema),
    defaultValues: {
      name: existingAliases.includes("default") ? "" : "default",
      collection_id: collections[0]?.id ?? "",
    },
  });
  const mut = useMutation({
    mutationFn: (v: z.infer<typeof aliasSchema>) =>
      putLibsecretAlias(storeName, resourceId, v.name, {
        collection_id: v.collection_id,
      }),
  });
  async function onSubmit(v: z.infer<typeof aliasSchema>) {
    setError(null);
    try {
      await mut.mutateAsync(v);
      await qc.invalidateQueries({
        queryKey: libsecretTreeKey(storeName, resourceId),
      });
      onClose();
    } catch (e) {
      setError(describeApiError(e));
    }
  }
  return (
    <Dialog open onOpenChange={(o) => !o && onClose()}>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>Set libsecret alias</DialogTitle>
          <DialogDescription>
            Aliases map a well-known name (commonly <code>default</code>)
            to a collection. libsecret clients call{" "}
            <code>ReadAlias</code> to resolve.
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
              name="name"
              render={({ field }) => (
                <FormItem>
                  <FormLabel>Alias name</FormLabel>
                  <FormControl>
                    <Input
                      autoFocus
                      autoCapitalize="off"
                      autoCorrect="off"
                      spellCheck={false}
                      placeholder="default"
                      {...field}
                    />
                  </FormControl>
                  <FormMessage />
                </FormItem>
              )}
            />
            <FormField
              control={form.control}
              name="collection_id"
              render={({ field }) => (
                <FormItem>
                  <FormLabel>Target collection</FormLabel>
                  <FormControl>
                    <Select {...field}>
                      {collections.map((c) => (
                        <option key={c.id} value={c.id}>
                          {c.label} ({c.id})
                        </option>
                      ))}
                    </Select>
                  </FormControl>
                  <FormMessage />
                </FormItem>
              )}
            />
            {error ? <Alert>{error}</Alert> : null}
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

function DeleteAliasDialog({
  storeName,
  resourceId,
  name,
  onClose,
}: {
  storeName: string;
  resourceId: string;
  name: string;
  onClose: () => void;
}) {
  const qc = useQueryClient();
  const mut = useMutation({
    mutationFn: () => deleteLibsecretAlias(storeName, resourceId, name),
    onSuccess: async () => {
      await qc.invalidateQueries({
        queryKey: libsecretTreeKey(storeName, resourceId),
      });
      onClose();
    },
  });
  return (
    <ConfirmDialog
      title="Delete alias"
      description={
        <>
          Removes alias <code>{name}</code> from resource{" "}
          <code>{resourceId}</code>. Collections it pointed at are
          untouched. This cannot be undone.
        </>
      }
      destructive
      confirmLabel="Delete"
      pendingLabel="Deleting…"
      onConfirm={() => mut.mutateAsync()}
      onClose={onClose}
    />
  );
}

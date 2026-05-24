import { useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { Plus } from "lucide-react";

import { Alert } from "@/components/ui/alert";
import { Button } from "@/components/ui/button";
import { type AdminLibsecretTreeResponse, fetchLibsecretTree } from "@/api/client";
import { describeApiError } from "@/lib/error-message";

import { AliasesPanel } from "./aliases-panel";
import { CollectionBlock } from "./collection-block";
import { CreateCollectionDialog } from "./dialogs/create-collection";
import { CreateItemDialog } from "./dialogs/create-item";
import { DeleteAliasDialog } from "./dialogs/delete-alias";
import { DeleteCollectionDialog } from "./dialogs/delete-collection";
import { DeleteItemDialog } from "./dialogs/delete-item";
import { RevealItemSecretDialog } from "./dialogs/reveal-item-secret";
import { SetAliasDialog } from "./dialogs/set-alias";
import { libsecretTreeKey } from "./internal";

export function LibsecretTree({
  storeId,
  resourceId,
}: {
  storeId: string;
  resourceId: string;
}) {
  const tree = useQuery<AdminLibsecretTreeResponse>({
    queryKey: libsecretTreeKey(storeId, resourceId),
    queryFn: () => fetchLibsecretTree(storeId, resourceId),
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
          storeId={storeId}
          resourceId={resourceId}
          existingIds={data.collections.map((c) => c.id)}
          onClose={() => setCreatingCollection(false)}
        />
      ) : null}
      {creatingItem ? (
        <CreateItemDialog
          storeId={storeId}
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
          storeId={storeId}
          resourceId={resourceId}
          collectionId={revealing.collectionId}
          itemId={revealing.itemId}
          onClose={() => setRevealing(null)}
        />
      ) : null}
      {deletingCollection ? (
        <DeleteCollectionDialog
          storeId={storeId}
          resourceId={resourceId}
          collectionId={deletingCollection}
          onClose={() => setDeletingCollection(null)}
        />
      ) : null}
      {deletingItem ? (
        <DeleteItemDialog
          storeId={storeId}
          resourceId={resourceId}
          collectionId={deletingItem.collectionId}
          itemId={deletingItem.itemId}
          onClose={() => setDeletingItem(null)}
        />
      ) : null}
      {creatingAlias ? (
        <SetAliasDialog
          storeId={storeId}
          resourceId={resourceId}
          collections={data.collections}
          existingAliases={Object.keys(data.aliases)}
          onClose={() => setCreatingAlias(false)}
        />
      ) : null}
      {deletingAlias ? (
        <DeleteAliasDialog
          storeId={storeId}
          resourceId={resourceId}
          name={deletingAlias}
          onClose={() => setDeletingAlias(null)}
        />
      ) : null}
    </section>
  );
}

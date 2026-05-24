import { ChevronRight, Eye, Plus, Trash2 } from "lucide-react";

import { Button } from "@/components/ui/button";
import type {
  AdminLibsecretCollectionView,
  AdminLibsecretItemView,
} from "@/api/client";

export function CollectionBlock({
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
          className="h-4 w-4 transition-transform group-[[open]]:rotate-90"
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

function ItemAttributes({
  attributes,
}: {
  attributes: AdminLibsecretItemView["attributes"];
}) {
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

import { useMemo } from "react";
import { Plus, Trash2 } from "lucide-react";

import { Button } from "@/components/ui/button";
import type {
  AdminLibsecretCollectionView,
  AdminLibsecretTreeResponse,
} from "@/api/client";

export function AliasesPanel({
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
      Object.entries(aliases)
        .filter(([, target]) => !known.has(target))
        .map(([name]) => name),
    );
  }, [aliases, collections]);

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
                  className="ml-1 rounded border border-amber-500/30 bg-amber-500/10 px-1.5 py-0.5 text-[0.65rem] text-(--color-warning)"
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

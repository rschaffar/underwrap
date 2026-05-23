import { ChevronRight, Eye, Pencil, Plus, Trash2 } from "lucide-react";

import { Button } from "@/components/ui/button";
import type { ResourceRecord } from "@/api/client";
import { describeCreator } from "@/lib/creator";
import { normalizeEpoch } from "@/lib/epoch";

import {
  SECRET_SLOTS,
  type SecretEntry,
  type SecretResourceGroup,
  type SecretSlot,
  resourceCount,
} from "./model";

interface RowActions {
  onReveal: (entry: SecretEntry) => void;
  onEdit: (entry: SecretEntry) => void;
  onDelete: (entry: SecretEntry) => void;
}

export function ResourceBlock({
  group,
  provenance,
  displayNameById,
  onAddEntry,
  onDeleteResource,
  onRenameResource,
  ...actions
}: {
  group: SecretResourceGroup;
  provenance: ResourceRecord | undefined;
  displayNameById: Map<string, string>;
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
  const provenanceEpoch = normalizeEpoch(provenance?.created_at);
  const provenanceDate =
    provenanceEpoch === null ? null : new Date(provenanceEpoch * 1000);
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
        <span
          className="font-mono text-[0.65rem] text-(--color-muted)/70"
          data-testid={`resource-id-${group.resource_id}`}
        >
          {group.resource_id}
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
        {provenance ? (
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
            onAdd={() => onAddEntry(slot)}
            {...actions}
          />
        ))}
      </div>
    </details>
  );
}

function SlotSection({
  slot,
  entries,
  onAdd,
  ...actions
}: {
  slot: SecretSlot;
  entries: SecretEntry[];
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

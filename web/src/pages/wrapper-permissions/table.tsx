import {
  ShieldCheck,
  ShieldOff,
  Hourglass,
  AlertTriangle,
  Trash2,
} from "lucide-react";

import { Button } from "@/components/ui/button";
import type {
  WrapperApprovalMode,
  WrapperPermission,
  WrapperResourcePairRecord,
} from "@/api/client";
import {
  epochToDate,
  formatEpochAbsolute,
  formatEpochRelative,
  formatEpochShort,
} from "@/lib/epoch";
import { describeCreator } from "@/lib/creator";

import {
  type WrapperResource,
  permissionKey,
  wrapperResourceKey,
} from "./derivations";

export function ResourceGroupedPairsTable({
  groups,
  clientInstances,
  displayNameById,
  resourceNameById,
  permissionIndex,
  onCellClick,
  onDeletePair,
}: {
  groups: { resource: string; pairs: WrapperResourcePairRecord[] }[];
  clientInstances: string[];
  displayNameById: Map<string, string>;
  resourceNameById: Map<string, string>;
  permissionIndex: Map<string, WrapperPermission>;
  onCellClick: (
    wr: WrapperResource,
    clientInstance: string,
    permission: WrapperPermission | null,
  ) => void;
  onDeletePair: (pair: WrapperResourcePairRecord) => void;
}) {
  return (
    <div className="overflow-x-auto rounded-xl border border-(--color-line)">
      <table className="min-w-full text-left text-sm">
        <thead className="bg-(--color-surface-0)/60 text-xs uppercase tracking-wide text-(--color-muted)">
          <tr>
            <th className="sticky left-0 z-10 bg-(--color-surface-0)/60 px-4 py-2">
              Resource
            </th>
            <th className="bg-(--color-surface-0)/60 px-4 py-2">Wrapper</th>
            <th className="bg-(--color-surface-0)/60 px-4 py-2 text-(--color-muted)">
              Last used
            </th>
            {clientInstances.map((c) => {
              const displayName = displayNameById.get(c) ?? c;
              return (
                <th key={c} className="px-3 py-2" title={c}>
                  <div className="font-medium text-(--color-fg)">
                    {displayName}
                  </div>
                </th>
              );
            })}
            <th className="px-3 py-2" />
          </tr>
        </thead>
        <tbody>
          {groups.flatMap((group) =>
            group.pairs.map((pair, indexWithinGroup) => {
              const wr: WrapperResource = {
                wrapper: pair.wrapper,
                resource_id: pair.resource_id,
              };
              const provenance = `Created ${formatEpochAbsolute(
                pair.created_at,
              )} by ${describeCreator(pair.created_by, displayNameById)}`;
              const isFirstInGroup = indexWithinGroup === 0;
              return (
                <tr
                  key={wrapperResourceKey(wr)}
                  className={
                    "align-middle " +
                    (isFirstInGroup ? "border-t-2 border-(--color-line)" : "")
                  }
                >
                  {isFirstInGroup ? (
                    <th
                      scope="rowgroup"
                      rowSpan={group.pairs.length}
                      className="sticky left-0 z-10 bg-(--color-surface-0)/40 px-4 py-3 text-left align-top font-medium text-(--color-fg)"
                      data-testid={`wrappers-resource-${group.resource}`}
                      title={group.resource}
                    >
                      <code className="rounded bg-(--color-surface-1) px-1.5 py-0.5 text-xs">
                        {resourceNameById.get(group.resource) ?? group.resource}
                      </code>
                    </th>
                  ) : null}
                  <td
                    className="px-4 py-3 font-medium text-(--color-fg)"
                    title={provenance}
                    data-testid={`wrappers-provenance-${pair.wrapper}-${pair.resource_id}`}
                  >
                    {pair.wrapper}
                  </td>
                  <td
                    className="px-4 py-3 text-xs text-(--color-muted)"
                    title={`Last used: ${formatEpochAbsolute(pair.last_used_at)}`}
                    data-testid={`wrappers-last-used-${pair.wrapper}-${pair.resource_id}`}
                  >
                    {pair.last_used_at == null ? (
                      <span className="opacity-60">never used</span>
                    ) : (
                      <>used {formatEpochRelative(pair.last_used_at)}</>
                    )}
                  </td>
                  {clientInstances.map((c) => {
                    const key = permissionKey(c, pair.wrapper, pair.resource_id);
                    const permission = permissionIndex.get(key) ?? null;
                    return (
                      <td key={key} className="px-3 py-3">
                        <button
                          type="button"
                          data-testid={`wrappers-cell-${pair.wrapper}-${pair.resource_id}-${c}`}
                          onClick={() => onCellClick(wr, c, permission)}
                          className="min-h-11 min-w-[5rem] rounded-md border border-(--color-line) bg-(--color-surface-1) px-2 py-1 text-xs hover:bg-(--color-surface-0) disabled:opacity-60"
                        >
                          <CellContent permission={permission} />
                        </button>
                      </td>
                    );
                  })}
                  <td className="px-3 py-3">
                    <Button
                      type="button"
                      variant="ghost"
                      size="sm"
                      aria-label={`Delete pair ${pair.wrapper} / ${
                        resourceNameById.get(pair.resource_id) ?? pair.resource_id
                      }`}
                      data-testid={`wrappers-delete-pair-${pair.wrapper}-${pair.resource_id}`}
                      onClick={() => onDeletePair(pair)}
                    >
                      <Trash2 className="h-4 w-4" aria-hidden />
                    </Button>
                  </td>
                </tr>
              );
            }),
          )}
        </tbody>
      </table>
    </div>
  );
}

/**
 * Absent permissions render as Locked — the same badge the explicit
 * `Locked` mode uses. This collapses two "deny" states (no row vs. a
 * row pinned to Locked) into a single visual: from the operator's
 * perspective, both deny reads, and the dialog still lets them upgrade
 * to a different mode.
 */
function CellContent({ permission }: { permission: WrapperPermission | null }) {
  if (!permission) {
    return <ModeBadge mode={{ kind: "locked" }} />;
  }
  return <ModeBadge mode={permission.mode ?? { kind: "locked" }} />;
}

export function ModeBadge({ mode }: { mode: WrapperApprovalMode }) {
  switch (mode.kind) {
    case "locked":
      return (
        <span
          data-testid="mode-locked"
          className="inline-flex items-center gap-1 rounded-md border border-rose-500/30 bg-rose-500/10 px-1.5 py-0.5 text-xs text-rose-200"
        >
          <ShieldOff className="h-3 w-3" aria-hidden /> locked
        </span>
      );
    case "unlocked":
      return (
        <span
          data-testid="mode-unlocked"
          className="inline-flex items-center gap-1 rounded-md border border-emerald-500/30 bg-emerald-500/10 px-1.5 py-0.5 text-xs text-emerald-200"
        >
          <ShieldCheck className="h-3 w-3" aria-hidden /> unlocked
        </span>
      );
    case "unlocked-until": {
      const date = epochToDate(mode.until_epoch);
      const expired = date.getTime() <= Date.now();
      if (expired) {
        return (
          <span
            data-testid="mode-expired"
            className="inline-flex items-center gap-1 rounded-md border border-rose-500/30 bg-rose-500/10 px-1.5 py-0.5 text-xs text-rose-200"
            title={`Expired ${date.toString()}`}
          >
            <ShieldOff className="h-3 w-3" aria-hidden /> expired{" "}
            {formatEpochShort(mode.until_epoch)}
          </span>
        );
      }
      return (
        <span
          data-testid="mode-unlocked-until"
          className="inline-flex items-center gap-1 rounded-md border border-amber-500/30 bg-amber-500/10 px-1.5 py-0.5 text-xs text-amber-200"
          title={`Expires ${date.toString()}`}
        >
          <Hourglass className="h-3 w-3" aria-hidden /> until{" "}
          {formatEpochShort(mode.until_epoch)}
        </span>
      );
    }
    case "unsafe-debug-until": {
      const date = epochToDate(mode.until_epoch);
      const expired = date.getTime() <= Date.now();
      if (expired) {
        return (
          <span
            data-testid="mode-unsafe-debug-expired"
            className="inline-flex items-center gap-1 rounded-md border border-rose-500/40 bg-rose-500/10 px-1.5 py-0.5 text-xs text-rose-200"
            title={`Unsafe-debug expired ${date.toString()}`}
          >
            <ShieldOff className="h-3 w-3" aria-hidden /> unsafe-debug expired
          </span>
        );
      }
      return (
        <span
          data-testid="mode-unsafe-debug-until"
          className="inline-flex items-center gap-1 rounded-md border border-amber-500/50 bg-amber-500/20 px-1.5 py-0.5 text-xs font-medium text-amber-100"
          title={`Unsafe-debug until ${date.toString()}`}
        >
          <AlertTriangle className="h-3 w-3" aria-hidden /> unsafe-debug until{" "}
          {formatEpochShort(mode.until_epoch)}
        </span>
      );
    }
  }
}

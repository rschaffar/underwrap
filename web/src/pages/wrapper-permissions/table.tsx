import { useId, type ReactNode } from "react";
import {
  ShieldCheck,
  ShieldOff,
  Hourglass,
  AlertTriangle,
  Trash2,
} from "lucide-react";

import { Button } from "@/components/ui/button";
import { Link } from "@tanstack/react-router";
import type {
  AdminClientInstanceListItem,
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
import { ClientAccessControls } from "./access-controls";

import {
  type WrapperResource,
  permissionKey,
  wrapperResourceKey,
} from "./derivations";

export function ResourceGroupedPairsTable({
  groups,
  clientInstances,
  visibleClients,
  displayNameById,
  resourceNameById,
  permissionIndex,
  clientDetails,
  clientMetadataReady,
  unknownOverrides,
  onOverrideUncertaintyChange,
  emptyMessage,
  storeId,
  onCellClick,
  onDeletePair,
}: {
  groups: { resource: string; pairs: WrapperResourcePairRecord[] }[];
  clientInstances: string[];
  visibleClients: ReadonlySet<string>;
  displayNameById: Map<string, string>;
  resourceNameById: Map<string, string>;
  permissionIndex: Map<string, WrapperPermission>;
  clientDetails: Map<string, AdminClientInstanceListItem>;
  clientMetadataReady: boolean;
  unknownOverrides: ReadonlySet<string>;
  onOverrideUncertaintyChange: (clientId: string, unknown: boolean) => void;
  emptyMessage: ReactNode;
  storeId: string;
  onCellClick: (
    wr: WrapperResource,
    clientInstance: string,
    permission: WrapperPermission | null,
  ) => void;
  onDeletePair: (pair: WrapperResourcePairRecord) => void;
}) {
  const tableId = useId();
  const columnHelpId = (clientId: string) => `${tableId}-${clientId}-access`;
  return (
    <div className="rounded-xl border border-(--color-line)">
      <p
        id={`${tableId}-legend`}
        className="p-3 text-left text-xs text-(--color-muted)"
      >
        Cells show saved individual settings, not a frontend authorization
        decision. Client overrides take precedence; unavailable stores and
        disabled identities still deny access. Missing rows deny in Individual
        settings. Overridden settings are muted and remain editable; unknown
        overrides are labelled separately. Unsafe debug always needs its
        separate timed approval.
      </p>
      {/* Keep absolutely positioned assistive text inside the table scroller. */}
      <div className="relative overflow-x-auto rounded-b-xl">
        <table
          aria-describedby={`${tableId}-legend`}
          className="min-w-full text-left text-sm"
        >
          <caption className="sr-only">
            Saved wrapper permissions by runtime client instance
          </caption>
          <thead className="bg-(--color-surface-0)/60 text-xs uppercase tracking-wide text-(--color-muted)">
            <tr>
              <th className="bg-(--color-surface-0) px-4 py-2 md:sticky md:left-0 md:z-10">
                Resource
              </th>
              <th className="bg-(--color-surface-0)/60 px-4 py-2">Wrapper</th>
              <th className="bg-(--color-surface-0)/60 px-4 py-2 text-(--color-muted)">
                Last used
              </th>
              {clientInstances.map((c) => {
                const displayName = displayNameById.get(c) ?? c;
                const client = clientMetadataReady
                  ? clientDetails.get(c)
                  : undefined;
                const override = client?.access_override;
                // Filtering hides a column without abandoning its pending/unknown control state.
                return (
                  <th
                    key={c}
                    scope="col"
                    hidden={!visibleClients.has(c)}
                    className="px-3 py-3 align-top"
                    data-testid={`wrappers-client-column-${c}`}
                  >
                    <div className="max-w-56 break-words font-medium normal-case tracking-normal text-(--color-fg)">
                      <Link
                        className="underline"
                        to="/client-instances"
                        title={c}
                        search={{ store: storeId, client: c }}
                      >
                        {displayName}
                      </Link>
                    </div>
                    {client ? (
                      <div className="mt-2">
                        <ClientAccessControls
                          instance={client}
                          onUncertaintyChange={onOverrideUncertaintyChange}
                        />
                      </div>
                    ) : null}
                    <div
                      id={columnHelpId(c)}
                      className="mt-2 max-w-56 text-xs font-normal normal-case tracking-normal"
                    >
                      {!clientMetadataReady
                        ? "Client state unavailable"
                        : !client
                          ? "No current mapping to this store"
                          : unknownOverrides.has(c)
                            ? "Override unknown — refresh client access before relying on it"
                            : client.status !== "active"
                              ? "Identity disabled — access denied"
                              : override?.kind === "locked"
                                ? "Lock all — normal/debug denied"
                                : override?.kind === "unlocked"
                                  ? `Unlock all — ${override.writable ? "writeback enabled" : "read-only"}; debug approval still required`
                                  : "Individual settings"}
                    </div>
                  </th>
                );
              })}
              <th className="px-3 py-2" />
            </tr>
          </thead>
          <tbody>
            {groups.length === 0 ? (
              <tr>
                <td
                  colSpan={visibleClients.size + 4}
                  className="border-t border-(--color-line) px-4 py-6 text-(--color-muted)"
                >
                  {emptyMessage}
                </td>
              </tr>
            ) : null}
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
                        className="bg-(--color-surface-0) px-4 py-3 text-left align-top font-medium text-(--color-fg) md:sticky md:left-0 md:z-10"
                        data-testid={`wrappers-resource-${group.resource}`}
                        title={group.resource}
                      >
                        <code className="rounded bg-(--color-surface-1) px-1.5 py-0.5 text-xs">
                          {resourceNameById.get(group.resource) ??
                            group.resource}
                        </code>
                        <div className="mt-1 text-xs font-normal text-(--color-muted)">
                          {group.resource}
                        </div>
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
                      const key = permissionKey(
                        c,
                        pair.wrapper,
                        pair.resource_id,
                      );
                      const permission = permissionIndex.get(key) ?? null;
                      const uncertain = unknownOverrides.has(c);
                      const override =
                        clientMetadataReady && !uncertain
                          ? clientDetails.get(c)?.access_override
                          : undefined;
                      const overridden =
                        override?.kind === "locked" ||
                        override?.kind === "unlocked";
                      return (
                        <td
                          key={key}
                          hidden={!visibleClients.has(c)}
                          className="px-3 py-3"
                          data-client-override={
                            uncertain
                              ? "unknown"
                              : (override?.kind ?? "unavailable")
                          }
                        >
                          <button
                            type="button"
                            data-testid={`wrappers-cell-${pair.wrapper}-${pair.resource_id}-${c}`}
                            onClick={() => onCellClick(wr, c, permission)}
                            aria-describedby={columnHelpId(c)}
                            data-overridden={overridden}
                            className={`min-h-11 w-full rounded-md border border-(--color-line) px-2 py-2 text-xs hover:bg-(--color-surface-0) ${overridden || uncertain ? "border-dashed bg-(--color-surface-0) text-(--color-muted)" : "bg-(--color-surface-1)"}`}
                          >
                            <CellContent
                              permission={permission}
                              accessOverride={override}
                              uncertain={uncertain}
                            />
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
                          resourceNameById.get(pair.resource_id) ??
                          pair.resource_id
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
    </div>
  );
}

function CellContent({
  permission,
  accessOverride,
  uncertain,
}: {
  permission: WrapperPermission | null;
  accessOverride?: AdminClientInstanceListItem["access_override"];
  uncertain: boolean;
}) {
  const kind = accessOverride?.kind;
  const overridden = kind === "locked" || kind === "unlocked";
  // Unlock all replaces ordinary admission, not the row's debug approval/expiry.
  const debugStillApplies =
    kind === "unlocked" && permission?.mode?.kind === "unsafe-debug-until";
  return (
    <span className="inline-flex flex-col items-start gap-1 text-left">
      {permission ? (
        <>
          <ModeBadge
            mode={permission.mode ?? { kind: "locked" }}
            muted={uncertain || (overridden && !debugStillApplies)}
          />
          <span className="text-(--color-muted)">
            Saved writeback: {permission.writable ? "on" : "off"}
          </span>
        </>
      ) : (
        <span
          data-testid="mode-missing"
          title="No saved row: access is denied in Individual settings."
        >
          No individual row
        </span>
      )}
      {uncertain ? (
        <span className="font-medium text-(--color-warning)">
          Override unknown
        </span>
      ) : overridden ? (
        <span
          className="text-(--color-muted)"
          data-testid="permission-override-note"
        >
          {kind === "locked"
            ? "Overridden · Lock all"
            : debugStillApplies
              ? "Normal access overridden"
              : "Overridden · Unlock all"}
        </span>
      ) : null}
      {debugStillApplies ? (
        <span className="font-medium text-(--color-warning)">
          Debug approval and expiry still apply
        </span>
      ) : null}
    </span>
  );
}

export function ModeBadge({
  mode,
  muted = false,
}: {
  mode: WrapperApprovalMode;
  muted?: boolean;
}) {
  const badgeClass = (tone: string) =>
    `inline-flex items-center gap-1 rounded-md border px-1.5 py-0.5 text-xs ${muted ? "border-(--color-line) bg-(--color-surface-0) text-(--color-muted)" : tone}`;
  switch (mode.kind) {
    case "locked":
      return (
        <span
          data-testid="mode-locked"
          data-muted={muted}
          className={badgeClass(
            "border-rose-500/30 bg-rose-500/10 text-(--color-danger)",
          )}
        >
          <ShieldOff className="h-3 w-3" aria-hidden /> locked
        </span>
      );
    case "unlocked":
      return (
        <span
          data-testid="mode-unlocked"
          data-muted={muted}
          className={badgeClass(
            "border-emerald-500/30 bg-emerald-500/10 text-(--color-success)",
          )}
        >
          <ShieldCheck className="h-3 w-3" aria-hidden /> unlocked
        </span>
      );
    case "unlocked-until": {
      const date = epochToDate(mode.until_epoch);
      const expired = Math.floor(Date.now() / 1000) > mode.until_epoch;
      if (expired) {
        return (
          <span
            data-testid="mode-expired"
            data-muted={muted}
            className={badgeClass(
              "border-rose-500/30 bg-rose-500/10 text-(--color-danger)",
            )}
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
          data-muted={muted}
          className={badgeClass(
            "border-amber-500/30 bg-amber-500/10 text-(--color-warning)",
          )}
          title={`Expires ${date.toString()}`}
        >
          <Hourglass className="h-3 w-3" aria-hidden /> until{" "}
          {formatEpochShort(mode.until_epoch)}
        </span>
      );
    }
    case "unsafe-debug-until": {
      const date = epochToDate(mode.until_epoch);
      const expired = Math.floor(Date.now() / 1000) > mode.until_epoch;
      if (expired) {
        return (
          <span
            data-testid="mode-unsafe-debug-expired"
            data-muted={muted}
            className={badgeClass(
              "border-rose-500/40 bg-rose-500/10 text-(--color-danger)",
            )}
            title={`Unsafe-debug expired ${date.toString()}`}
          >
            <ShieldOff className="h-3 w-3" aria-hidden /> unsafe-debug expired
          </span>
        );
      }
      return (
        <span
          data-testid="mode-unsafe-debug-until"
          data-muted={muted}
          className={badgeClass(
            "border-amber-500/50 bg-amber-500/20 font-medium text-(--color-warning)",
          )}
          title={`Unsafe-debug until ${date.toString()}`}
        >
          <AlertTriangle className="h-3 w-3" aria-hidden /> unsafe-debug until{" "}
          {formatEpochShort(mode.until_epoch)}
        </span>
      );
    }
  }
}

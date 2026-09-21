import { useEffect, useId, useRef, useState } from "react";
import { useQueryClient } from "@tanstack/react-query";

import {
  type AdminClientInstanceListItem,
  type AdminClientInstancesListResponse,
  fetchClientInstances,
  putClientInstanceUpdate,
} from "@/api/client";
import { ConfirmDialog } from "@/components/confirm-dialog";
import { Alert } from "@/components/ui/alert";
import { Button } from "@/components/ui/button";
import { Label } from "@/components/ui/label";
import { Select } from "@/components/ui/select";
import { describeApiError } from "@/lib/error-message";

type AccessOverride = AdminClientInstanceListItem["access_override"];
const clientsKey = ["admin", "client-instances"] as const;
const permissionsKey = ["admin", "wrapper-permissions-view"] as const;

export function ClientAccessControls({
  instance,
  onUncertaintyChange,
}: {
  instance: AdminClientInstanceListItem;
  onUncertaintyChange?: (clientId: string, unknown: boolean) => void;
}) {
  const queryClient = useQueryClient();
  const id = useId();
  const inFlight = useRef(false);
  const confirmingRef = useRef(false);
  const unknownRef = useRef(false);
  const mounted = useRef(false);
  const [pending, setPending] = useState(false);
  const [confirming, setConfirming] = useState(false);
  const [unknown, setUnknown] = useState(false);
  const [submitError, setSubmitError] = useState<string | null>(null);
  const [refreshError, setRefreshError] = useState<string | null>(null);
  // Keep an acknowledged response visible even before the parent receives the
  // cache update. A later parent snapshot wins once this operation has settled.
  const [acknowledged, setAcknowledged] = useState<{
    source: AdminClientInstanceListItem;
    item: AdminClientInstanceListItem;
  } | null>(null);
  const current =
    acknowledged && (pending || acknowledged.source === instance)
      ? acknowledged.item
      : instance;
  const access = current.access_override;

  // Column presentation must not describe a lost response as a known override.
  // Drop this component's uncertainty marker when its column is removed.
  useEffect(() => {
    mounted.current = true;
    onUncertaintyChange?.(instance.id, unknownRef.current);
    return () => {
      mounted.current = false;
      onUncertaintyChange?.(instance.id, false);
    };
  }, [instance.id, onUncertaintyChange]);

  function setStateUnknown(value: boolean) {
    unknownRef.current = value;
    setUnknown(value);
    if (mounted.current) onUncertaintyChange?.(instance.id, value);
  }

  function closeConfirmation() {
    if (inFlight.current) return;
    confirmingRef.current = false;
    setConfirming(false);
  }

  async function invalidateViews() {
    // Callback/cache failures deliberately remain observable rejections, outside
    // both request catches. Never describe them as a failed access update.
    await Promise.all([
      queryClient.invalidateQueries(
        { queryKey: clientsKey },
        { throwOnError: true },
      ),
      queryClient.invalidateQueries(
        { queryKey: permissionsKey },
        { throwOnError: true },
      ),
    ]);
  }

  async function reconcile(): Promise<boolean> {
    setStateUnknown(true);
    setRefreshError(null);
    // Cancel an older list read before making a new, uncached request. Neither
    // query defaults nor mutation defaults may silently retry this operation.
    await queryClient.cancelQueries({ queryKey: clientsKey });
    let list: AdminClientInstancesListResponse;
    try {
      list = await fetchClientInstances();
    } catch (error) {
      setRefreshError(describeApiError(error));
      return false;
    }
    const refreshed = list.items.find((item) => item.id === instance.id);
    if (!refreshed) {
      setRefreshError(
        "This client instance is not in the refreshed client list.",
      );
      return false;
    }
    setAcknowledged({ source: instance, item: refreshed });
    queryClient.setQueryData(clientsKey, list);
    setStateUnknown(false);
    return true;
  }

  async function changeAccess(next: AccessOverride) {
    if (inFlight.current || unknownRef.current) return;
    if (
      next.kind === access.kind &&
      (next.kind !== "unlocked" ||
        (access.kind === "unlocked" && next.writable === access.writable))
    ) {
      return;
    }
    inFlight.current = true;
    setPending(true);
    setAcknowledged({ source: instance, item: current });
    setSubmitError(null);
    setRefreshError(null);
    try {
      await queryClient.cancelQueries({ queryKey: clientsKey });
      let updated: AdminClientInstanceListItem;
      try {
        updated = await putClientInstanceUpdate(instance.id, {
          access_override: next,
        });
      } catch (error) {
        setSubmitError(describeApiError(error));
        confirmingRef.current = false;
        setConfirming(false);
        // A lost response may hide a committed change. Do not present the old
        // selection as current or permit another change until a fresh read wins.
        if (await reconcile()) await invalidateViews();
        return;
      }
      setAcknowledged({ source: instance, item: updated });
      queryClient.setQueryData<AdminClientInstancesListResponse>(
        clientsKey,
        (previous) =>
          previous
            ? {
                ...previous,
                items: previous.items.map((item) =>
                  item.id === updated.id ? updated : item,
                ),
              }
            : undefined,
      );
      confirmingRef.current = false;
      setConfirming(false);
      await invalidateViews();
    } finally {
      inFlight.current = false;
      setPending(false);
    }
  }

  async function retryRefresh() {
    if (inFlight.current || !unknownRef.current) return;
    inFlight.current = true;
    setPending(true);
    try {
      if (await reconcile()) {
        setSubmitError(null);
        await invalidateViews();
      }
    } finally {
      inFlight.current = false;
      setPending(false);
    }
  }

  function selectMode(kind: string) {
    if (inFlight.current || unknownRef.current || confirmingRef.current) return;
    if (kind === access.kind) return;
    if (kind === "unlocked") {
      confirmingRef.current = true;
      setConfirming(true);
      setSubmitError(null);
    } else if (kind === "individual" || kind === "locked") {
      void changeAccess({ kind });
    }
  }

  return (
    <div
      className="min-w-52 space-y-2 text-left font-normal normal-case tracking-normal"
      role="group"
      aria-label={`Access override for ${current.display_name}`}
      aria-busy={pending}
    >
      <Label htmlFor={`${id}-access`}>Client access</Label>
      <Select
        id={`${id}-access`}
        aria-describedby={`${id}-help`}
        value={unknown ? "unknown" : access.kind}
        disabled={pending || unknown || confirming}
        onChange={(event) => selectMode(event.target.value)}
      >
        {unknown ? <option value="unknown">State unknown</option> : null}
        <option value="individual">Individual settings</option>
        <option value="locked">Lock all</option>
        <option value="unlocked">Unlock all</option>
      </Select>
      {!unknown && access.kind === "unlocked" ? (
        <label className="flex items-center gap-2 text-sm">
          <input
            type="checkbox"
            checked={access.writable}
            disabled={pending || confirming}
            onChange={(event) => {
              if (confirmingRef.current) return;
              void changeAccess({
                kind: "unlocked",
                writable: event.target.checked,
              });
            }}
          />
          Allow files/libsecret writeback
        </label>
      ) : null}
      <p id={`${id}-help`} className="sr-only">
        Changes apply on subsequent opens and finalizes, not as immediate
        process revocation. Writable staging is not persistence permission; the
        backend remains the authority.
      </p>
      {pending ? (
        <p role="status" className="text-xs text-(--color-muted)">
          {unknown ? "Refreshing client access…" : "Updating client access…"}
        </p>
      ) : null}
      {submitError ? <Alert>{submitError}</Alert> : null}
      {unknown ? (
        <Alert>
          <p>
            Client access state is unknown. Refresh before making further
            changes.
          </p>
          {refreshError ? <p>{refreshError}</p> : null}
          <Button
            type="button"
            variant="ghost"
            disabled={pending}
            onClick={() => void retryRefresh()}
          >
            Retry refresh
          </Button>
        </Alert>
      ) : null}
      {confirming ? (
        <ConfirmDialog
          title={`Unlock all: ${current.display_name}`}
          description={`Apply a client-wide override for ${current.display_name} in store ${current.current_store_name}?`}
          body={
            <div className="space-y-2 text-sm">
              <p>
                All registered wrapper/resource pairs are included, even those
                with missing, locked, or expired ordinary permission rows. Saved
                individual rows stay unchanged when this override is enabled or
                removed.
              </p>
              <p>
                Unsafe debug still needs active timed per-pair approval; this
                override does not grant it.
              </p>
              <p>
                The initial override is read-only. Files/libsecret writeback
                needs the separate writable flag; env cannot be mutated.
                Disabled identities, locked stores, and unregistered pairs are
                not bypassed.
              </p>
            </div>
          }
          destructive
          confirmLabel="Unlock all read-only"
          pendingLabel="Unlocking…"
          pending={pending}
          submitError={null}
          onConfirm={() => changeAccess({ kind: "unlocked", writable: false })}
          onClose={closeConfirmation}
        />
      ) : null}
    </div>
  );
}

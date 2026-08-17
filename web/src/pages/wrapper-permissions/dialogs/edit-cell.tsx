import { useState } from "react";
import { AlertTriangle } from "lucide-react";
import { useMutation, useQueryClient } from "@tanstack/react-query";

import { AdminDialog } from "@/components/admin-dialog";
import { ConfirmDangerDialog } from "@/components/confirm-danger-dialog";
import { Alert } from "@/components/ui/alert";
import { Button } from "@/components/ui/button";
import { DialogFooter } from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import {
  type WrapperApprovalMode,
  type WrapperPermission,
  deleteWrapperPermission,
  putWrapperPermission,
} from "@/api/client";
import { describeApiError } from "@/lib/error-message";
import { epochToDate, toDatetimeLocal } from "@/lib/epoch";

export function EditCellDialog({
  storeId,
  clientInstance,
  clientInstanceDisplayName,
  wrapper,
  resourceId,
  resourceName,
  existing,
  onClose,
}: {
  storeId: string;
  clientInstance: string;
  clientInstanceDisplayName: string;
  wrapper: string;
  resourceId: string;
  resourceName: string;
  existing: WrapperPermission | null;
  onClose: () => void;
}) {
  const qc = useQueryClient();
  const [kind, setKind] = useState<WrapperApprovalMode["kind"]>(
    existing?.mode?.kind ?? "locked",
  );
  const [untilLocal, setUntilLocal] = useState<string>(() => {
    if (
      existing?.mode?.kind === "unlocked-until" ||
      existing?.mode?.kind === "unsafe-debug-until"
    ) {
      return toDatetimeLocal(epochToDate(existing.mode.until_epoch));
    }
    return toDatetimeLocal(new Date(Date.now() + 30 * 60 * 1000));
  });
  const [writable, setWritable] = useState<boolean>(existing?.writable ?? false);
  const [reason, setReason] = useState<string>(existing?.reason ?? "");
  const [submitError, setSubmitError] = useState<string | null>(null);
  // A fresh unsafe-debug selection gets one exact candidate deadline. The
  // confirmation displays this same value before applying it to the form.
  const [pendingUnsafeDebug, setPendingUnsafeDebug] = useState<string | null>(
    null,
  );

  // `unsafe-debug-until` shares the deadline picker with `unlocked-until`.
  const showDeadline = kind === "unlocked-until" || kind === "unsafe-debug-until";

  function onKindChange(next: WrapperApprovalMode["kind"]) {
    if (next === "unsafe-debug-until" && kind !== "unsafe-debug-until") {
      setPendingUnsafeDebug(
        toDatetimeLocal(new Date(Date.now() + 15 * 60 * 1000)),
      );
      return;
    }
    setKind(next);
  }

  function armUnsafeDebug() {
    if (pendingUnsafeDebug === null) return;
    setUntilLocal(pendingUnsafeDebug);
    setKind("unsafe-debug-until");
    setPendingUnsafeDebug(null);
  }

  const upsert = useMutation({
    mutationFn: (mode: WrapperApprovalMode) =>
      putWrapperPermission(
        storeId,
        clientInstance,
        wrapper,
        resourceId,
        {
          mode,
          writable,
          reason: reason.trim() === "" ? undefined : reason.trim(),
        },
      ),
    onSuccess: async () => {
      await qc.invalidateQueries({
        queryKey: ["admin", "wrapper-permissions-view", storeId],
      });
      onClose();
    },
    onError: (err) => setSubmitError(describeApiError(err)),
  });
  const remove = useMutation({
    mutationFn: () =>
      deleteWrapperPermission(storeId, clientInstance, wrapper, resourceId),
    onSuccess: async () => {
      await qc.invalidateQueries({
        queryKey: ["admin", "wrapper-permissions-view", storeId],
      });
      onClose();
    },
    onError: (err) => setSubmitError(describeApiError(err)),
  });

  function onSave() {
    setSubmitError(null);
    let mode: WrapperApprovalMode;
    switch (kind) {
      case "locked":
        mode = { kind: "locked" };
        break;
      case "unlocked":
        mode = { kind: "unlocked" };
        break;
      case "unlocked-until":
      case "unsafe-debug-until": {
        const until = new Date(untilLocal).getTime();
        if (Number.isNaN(until)) {
          setSubmitError("Pick a valid deadline.");
          return;
        }
        const until_epoch = Math.floor(until / 1000);
        mode =
          kind === "unlocked-until"
            ? { kind: "unlocked-until", until_epoch }
            : { kind: "unsafe-debug-until", until_epoch };
        break;
      }
    }
    upsert.mutate(mode);
  }

  return (
    <>
    <AdminDialog
      open
      onClose={onClose}
      title={`${wrapper} / ${resourceName} → ${clientInstanceDisplayName}`}
      description={
        <>
          Set the mode{" "}
          <span title={clientInstance}>{clientInstanceDisplayName}</span>{" "}
          uses when calling{" "}
          <code className="rounded bg-(--color-surface-1) px-1 py-0.5">
            {wrapper}
          </code>{" "}
          for resource{" "}
          <code
            className="rounded bg-(--color-surface-1) px-1 py-0.5"
            title={resourceId}
          >
            {resourceName}
          </code>
          . The mode is checked server-side on every read.
        </>
      }
    >
      <div className="space-y-4">
          <fieldset
            className="flex flex-wrap gap-2"
            data-testid="mode-radio-group"
          >
            <legend className="sr-only">Mode</legend>
            {(
              [
                "locked",
                "unlocked",
                "unlocked-until",
                "unsafe-debug-until",
              ] as const
            ).map((k) => {
              const danger = k === "unsafe-debug-until";
              const selectedClass = danger
                ? "border-amber-500/50 bg-amber-500/10 text-amber-200"
                : "border-emerald-500/40 bg-emerald-500/10 text-emerald-200";
              return (
                <label
                  key={k}
                  className={
                    "inline-flex cursor-pointer items-center gap-2 rounded-md border px-3 py-1.5 text-sm " +
                    (kind === k
                      ? selectedClass
                      : "border-(--color-line) bg-(--color-surface-1) text-(--color-fg)")
                  }
                >
                  <input
                    type="radio"
                    name="mode"
                    value={k}
                    data-testid={`mode-radio-${k}`}
                    className="sr-only"
                    checked={kind === k}
                    onChange={() => onKindChange(k)}
                  />
                  {danger ? (
                    <span className="inline-flex items-center gap-1">
                      <AlertTriangle className="h-3 w-3" aria-hidden />
                      unsafe-debug
                    </span>
                  ) : (
                    k
                  )}
                </label>
              );
            })}
          </fieldset>
          {showDeadline ? (
            <div className="space-y-1.5">
              <Label htmlFor="until-local">Deadline (local time)</Label>
              <Input
                id="until-local"
                type="datetime-local"
                value={untilLocal}
                onChange={(e) => setUntilLocal(e.target.value)}
              />
            </div>
          ) : null}
          <label className="inline-flex cursor-pointer items-center gap-2 text-sm text-(--color-fg)">
            <input
              type="checkbox"
              data-testid="permission-writable"
              checked={writable}
              onChange={(e) => setWritable(e.target.checked)}
            />
            Allow wrapper-side writes for this resource (files / libsecret slots)
          </label>
          <div className="space-y-1.5">
            <Label htmlFor="reason">Reason (optional)</Label>
            <Input
              id="reason"
              value={reason}
              onChange={(e) => setReason(e.target.value)}
            />
          </div>
          {submitError ? <Alert>{submitError}</Alert> : null}
        </div>
        <DialogFooter className="sm:justify-between">
          {existing ? (
            <Button
              type="button"
              variant="destructive"
              size="sm"
              onClick={() => remove.mutate()}
              disabled={remove.isPending}
              data-testid="permission-remove"
            >
              {remove.isPending ? "Removing…" : "Remove permission"}
            </Button>
          ) : (
            <span />
          )}
          <div className="flex gap-2">
            <Button type="button" variant="ghost" onClick={onClose}>
              Cancel
            </Button>
            <Button
              type="button"
              onClick={onSave}
              disabled={upsert.isPending}
              data-testid="permission-save"
            >
              {upsert.isPending ? "Saving…" : "Save"}
            </Button>
          </div>
        </DialogFooter>
    </AdminDialog>
    {pendingUnsafeDebug !== null ? (
      <ConfirmDangerDialog
        title="Enable unsafe debug?"
        confirmLabel="Yes, select unsafe debug"
        onConfirm={armUnsafeDebug}
        onCancel={() => setPendingUnsafeDebug(null)}
        description={
          <span
            className="block max-h-[65vh] space-y-3 overflow-y-auto pr-2 text-left"
            data-testid="unsafe-debug-security-warning"
          >
            <span className="block">
              <strong>Exact backend authority:</strong> select{" "}
              <code>UnsafeDebugUntil</code> for runtime client{" "}
              <strong>{clientInstanceDisplayName}</strong> (id{" "}
              <code>{clientInstance}</code>), store <code>{storeId}</code>,
              wrapper <code>{wrapper}</code>, and resource{" "}
              <strong>{resourceName}</strong> (id <code>{resourceId}</code>).
            </span>
            <span className="block">
              <strong>Local reach:</strong> Any local user can invoke the
              system-wide frontend instance entry while this permission is
              active. Backend permission is not Unix-user authorization, and{" "}
              <code>allowedUsers</code> installs PATH shims only. If selected
              users or a per-invocation prompt are required, put sudo, polkit,
              or another OS-level gate in front before Save.
            </span>
            <span className="block">
              The conventional command is{" "}
              <code>underwrap-&lt;instance&gt; debug {wrapper}</code>. This backend
              UI does not know the frontend instance name; verify the matching
              frontend instance before Save.
            </span>
            <span className="block">
              <strong>Full projection disclosure:</strong>{" "}
              <code>underwrap-debug-tool</code> can show an environment summary,
              open an interactive shell, or execute an arbitrary command with
              the complete configured env, files, and libsecret projection. It
              can print, copy through accessible paths, or transmit that
              projection. underwrap is not a behavioral sandbox.
            </span>
            {writable ? (
              <span className="block" data-testid="unsafe-debug-writable-effect">
                <strong>Writable is enabled:</strong> files and libsecret
                changes can be submitted, written back on successful
                finalization, and persisted for future invocations, affecting
                their integrity. Env remains backend-owned and immutable.
              </span>
            ) : (
              <span className="block" data-testid="unsafe-debug-readonly-effect">
                <strong>Writable is disabled:</strong> underwrap does not persist
                staged files or libsecret changes, but full target access and
                disclosure are unchanged. Env remains backend-owned and
                immutable.
              </span>
            )}
            <span className="block">
              <strong>Pending local expiry:</strong>{" "}
              <time
                dateTime={pendingUnsafeDebug}
                data-testid="unsafe-debug-pending-expiry"
              >
                {pendingUnsafeDebug}
              </time>{" "}
              (15-minute default). Confirming only copies this exact value into
              the form. Unsafe debug is not active until you press Save; you can
              review or change the deadline first. The backend rechecks expiry
              on every request.
            </span>
          </span>
        }
      />
    ) : null}
    </>
  );
}

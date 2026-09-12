import { useRef, useState } from "react";
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
  type AdminClientInstanceListItem,
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
  accessOverride,
  onClose,
}: {
  storeId: string;
  clientInstance: string;
  clientInstanceDisplayName: string;
  wrapper: string;
  resourceId: string;
  resourceName: string;
  existing: WrapperPermission | null;
  accessOverride?: AdminClientInstanceListItem["access_override"];
  onClose: () => void;
}) {
  const qc = useQueryClient();
  const overrideHelp = !accessOverride
    ? "Client access metadata is unavailable. Do not infer effective access or read-only behavior from this saved row."
    : accessOverride.kind === "locked"
      ? "Lock all denies this client's normal and debug requests. Saving this individual row does not unlock the client."
      : accessOverride.kind === "unlocked"
        ? `Unlock all overrides ordinary access and the saved writable flag (${accessOverride.writable ? "writeback enabled" : "read-only"}). Unsafe debug still requires this pair's active timed approval. Returning to Individual settings restores the saved row's effect.`
        : "Individual settings uses this saved mode and writable flag. The backend remains the authority and other admission checks still apply.";
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
  const [writable, setWritable] = useState<boolean>(
    existing?.writable ?? false,
  );
  const [reason, setReason] = useState<string>(existing?.reason ?? "");
  const [submitError, setSubmitError] = useState<string | null>(null);
  const inFlight = useRef(false);
  const [action, setAction] = useState<"save" | "remove" | null>(null);
  const busy = action !== null;
  // A fresh unsafe-debug selection gets one exact candidate deadline. The
  // confirmation displays this same value before applying it to the form.
  const [pendingUnsafeDebug, setPendingUnsafeDebug] = useState<string | null>(
    null,
  );

  // `unsafe-debug-until` shares the deadline picker with `unlocked-until`.
  const showDeadline =
    kind === "unlocked-until" || kind === "unsafe-debug-until";

  function onKindChange(next: WrapperApprovalMode["kind"]) {
    if (inFlight.current) return;
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

  type Operation =
    | { kind: "remove" }
    | {
        kind: "save";
        body: Parameters<typeof putWrapperPermission>[4];
      };
  const mutation = useMutation({
    mutationFn: async (operation: Operation) => {
      if (operation.kind === "remove") {
        await deleteWrapperPermission(
          storeId,
          clientInstance,
          wrapper,
          resourceId,
        );
      } else {
        await putWrapperPermission(
          storeId,
          clientInstance,
          wrapper,
          resourceId,
          operation.body,
        );
      }
    },
  });
  async function submit(operation: Operation) {
    if (inFlight.current) return;
    inFlight.current = true;
    setAction(operation.kind);
    setSubmitError(null);
    try {
      try {
        await mutation.mutateAsync(operation);
      } catch (error) {
        setSubmitError(describeApiError(error));
        return;
      }
      // Request errors are handled above; callback/invalidation failures stay observable.
      await qc.invalidateQueries(
        { queryKey: ["admin", "wrapper-permissions-view", storeId] },
        { throwOnError: true },
      );
      onClose();
    } finally {
      inFlight.current = false;
      setAction(null);
    }
  }

  function onSave() {
    if (inFlight.current) return;
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
    return submit({
      kind: "save",
      body: {
        mode,
        writable,
        reason: reason.trim() === "" ? undefined : reason.trim(),
      },
    });
  }

  return (
    <>
      <AdminDialog
        open
        onClose={() => {
          if (!inFlight.current) onClose();
        }}
        nonDismissible={busy}
        title={`${wrapper} / ${resourceName} → ${clientInstanceDisplayName}`}
        description={
          <>
            Edit the saved individual mode for{" "}
            <span title={clientInstance}>{clientInstanceDisplayName}</span> when
            calling{" "}
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
            . Current access is checked server-side on every open and finalize.
          </>
        }
      >
        <p className="my-3 rounded-md border border-(--color-line) p-3 text-sm text-(--color-muted)">
          {overrideHelp}
        </p>
        <fieldset
          disabled={busy}
          className="min-w-0 space-y-4"
          aria-label="Individual permission settings"
        >
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
                ? "border-amber-500/50 bg-amber-500/10 text-(--color-warning)"
                : "border-emerald-500/40 bg-emerald-500/10 text-(--color-success)";
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
            Saved: Allow wrapper-side writes for this resource (files /
            libsecret slots)
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
        </fieldset>
        <DialogFooter className="sm:justify-between">
          {existing ? (
            <Button
              type="button"
              variant="destructive"
              size="sm"
              onClick={() => submit({ kind: "remove" })}
              disabled={busy}
              data-testid="permission-remove"
            >
              {action === "remove" ? "Removing…" : "Remove permission"}
            </Button>
          ) : (
            <span />
          )}
          <div className="flex gap-2">
            <Button
              type="button"
              variant="ghost"
              disabled={busy}
              onClick={() => {
                if (!inFlight.current) onClose();
              }}
            >
              Cancel
            </Button>
            <Button
              type="button"
              onClick={onSave}
              disabled={busy}
              data-testid="permission-save"
            >
              {action === "save" ? "Saving…" : "Save"}
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
                <code>underwrap-&lt;instance&gt; debug {wrapper}</code>. This
                backend UI does not know the frontend instance name; verify the
                matching frontend instance before Save.
              </span>
              <span className="block">
                <strong>Full projection disclosure:</strong>{" "}
                <code>underwrap-debug-tool</code> can show an environment
                summary, open an interactive shell, or execute an arbitrary
                command with the complete configured env, files, and libsecret
                projection. It can print, copy through accessible paths, or
                transmit that projection. underwrap is not a behavioral sandbox.
              </span>
              <span className="block">{overrideHelp}</span>
              {(
                accessOverride?.kind === "unlocked"
                  ? accessOverride.writable
                  : writable
              ) ? (
                <span
                  className="block"
                  data-testid="unsafe-debug-writable-effect"
                >
                  <strong>Writable is enabled:</strong> files and libsecret
                  changes can be submitted, written back on successful
                  finalization, and persisted for future invocations, affecting
                  their integrity. Env remains backend-owned and immutable.
                </span>
              ) : (
                <span
                  className="block"
                  data-testid="unsafe-debug-readonly-effect"
                >
                  <strong>
                    Writable is disabled in the displayed setting:
                  </strong>{" "}
                  this setting does not authorize persistence of staged files or
                  libsecret changes; client overrides and current backend checks
                  still apply. Full target access and disclosure are unchanged.
                  Env remains backend-owned and immutable.
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
                (15-minute default). Confirming only copies this exact value
                into the form. Save submits this selection; confirming does not
                change backend policy. Existing permissions and overrides remain
                in effect until changed or expired. You can review or change the
                deadline first. The backend rechecks expiry on every request.
              </span>
            </span>
          }
        />
      ) : null}
    </>
  );
}

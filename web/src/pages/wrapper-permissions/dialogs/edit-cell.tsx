import { useState } from "react";
import { useMutation, useQueryClient } from "@tanstack/react-query";

import { Alert } from "@/components/ui/alert";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import {
  postWrapperPermissionDelete,
  postWrapperPermissionUpsert,
  type WrapperApprovalMode,
  type WrapperPermission,
} from "@/api/client";
import { describeApiError } from "@/lib/error-message";
import { toDatetimeLocal } from "@/lib/epoch";

import type { ModeKind } from "../derivations";

export function EditCellDialog({
  storeName,
  clientInstance,
  clientInstanceDisplayName,
  wrapper,
  resourceId,
  resourceName,
  existing,
  onClose,
}: {
  storeName: string;
  clientInstance: string;
  clientInstanceDisplayName: string;
  wrapper: string;
  resourceId: string;
  resourceName: string;
  existing: WrapperPermission | null;
  onClose: () => void;
}) {
  const qc = useQueryClient();
  const [kind, setKind] = useState<ModeKind>(
    existing ? existing.mode.kind : "locked",
  );
  const [untilLocal, setUntilLocal] = useState<string>(() => {
    if (existing && existing.mode.kind === "unlocked-until") {
      return toDatetimeLocal(new Date(existing.mode.until_epoch * 1000));
    }
    return toDatetimeLocal(new Date(Date.now() + 30 * 60 * 1000));
  });
  const [writable, setWritable] = useState<boolean>(existing?.writable ?? false);
  const [reason, setReason] = useState<string>(existing?.reason ?? "");
  const [submitError, setSubmitError] = useState<string | null>(null);

  const upsert = useMutation({
    mutationFn: (mode: WrapperApprovalMode) =>
      postWrapperPermissionUpsert({
        store: storeName,
        permission: {
          client_instance: clientInstance,
          wrapper,
          resource_id: resourceId,
          mode,
          writable,
          reason: reason.trim() === "" ? undefined : reason.trim(),
        },
      }),
    onSuccess: async () => {
      await qc.invalidateQueries({ queryKey: ["admin", "wrapper-permissions", storeName] });
      onClose();
    },
    onError: (err) => setSubmitError(describeApiError(err)),
  });
  const remove = useMutation({
    mutationFn: () =>
      postWrapperPermissionDelete({
        store: storeName,
        client_instance: clientInstance,
        wrapper,
        resource_id: resourceId,
      }),
    onSuccess: async () => {
      await qc.invalidateQueries({ queryKey: ["admin", "wrapper-permissions", storeName] });
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
      case "unlocked-until": {
        const until = new Date(untilLocal).getTime();
        if (Number.isNaN(until)) {
          setSubmitError("Pick a valid deadline.");
          return;
        }
        mode = {
          kind: "unlocked-until",
          until_epoch: Math.floor(until / 1000),
        };
        break;
      }
    }
    upsert.mutate(mode);
  }

  return (
    <Dialog open onOpenChange={(o) => !o && onClose()}>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>
            {wrapper} / {resourceName} → {clientInstanceDisplayName}
          </DialogTitle>
          <DialogDescription>
            Set the mode{" "}
            <span title={clientInstance}>{clientInstanceDisplayName}</span>{" "}
            uses when calling{" "}
            <code className="rounded bg-(--color-surface-1) px-1 py-0.5">{wrapper}</code>{" "}
            for resource{" "}
            <code
              className="rounded bg-(--color-surface-1) px-1 py-0.5"
              title={resourceId}
            >
              {resourceName}
            </code>
            . The mode is checked server-side on every read.
          </DialogDescription>
        </DialogHeader>
        <div className="space-y-4">
          <fieldset
            className="flex flex-wrap gap-2"
            data-testid="mode-radio-group"
          >
            {(["locked", "unlocked", "unlocked-until"] as const).map(
              (k) => (
                <label
                  key={k}
                  className={
                    "inline-flex cursor-pointer items-center gap-2 rounded-md border px-3 py-1.5 text-sm " +
                    (kind === k
                      ? "border-emerald-500/40 bg-emerald-500/10 text-emerald-200"
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
                    onChange={() => setKind(k)}
                  />
                  {k}
                </label>
              ),
            )}
          </fieldset>
          {kind === "unlocked-until" ? (
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
      </DialogContent>
    </Dialog>
  );
}

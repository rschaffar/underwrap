import { useState } from "react";
import { useMutation } from "@tanstack/react-query";

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
import { Select } from "@/components/ui/select";
import {
  type AdminClientInstanceDetailResponse,
  type AdminStoresListResponse,
  putClientInstanceUpdate,
} from "@/api/client";
import { describeApiError } from "@/lib/error-message";

export function EditClientInstanceDialog({
  instance,
  stores,
  onClose,
  onSuccess,
}: {
  instance: AdminClientInstanceDetailResponse;
  stores: AdminStoresListResponse["items"];
  onClose: () => void;
  onSuccess: () => Promise<void>;
}) {
  const [displayName, setDisplayName] = useState(instance.display_name);
  const [storeId, setStoreId] = useState(instance.current_store_id);
  const [certPem, setCertPem] = useState("");
  const [autoCreate, setAutoCreate] = useState(
    instance.auto_create_wrapper_assets,
  );
  const [submitError, setSubmitError] = useState<string | null>(null);

  const mut = useMutation({
    mutationFn: (body: {
      display_name?: string;
      current_store_id?: string;
      cert_pem?: string;
      auto_create_wrapper_assets?: boolean;
    }) => putClientInstanceUpdate(instance.id, body),
    onSuccess: async () => {
      setSubmitError(null);
      await onSuccess();
    },
    onError: (err) => setSubmitError(describeApiError(err)),
  });

  const trimmedDisplayName = displayName.trim();
  const trimmedCert = certPem.trim();
  const displayNameChanged =
    trimmedDisplayName !== "" && trimmedDisplayName !== instance.display_name;
  const storeChanged = storeId !== instance.current_store_id;
  const certChanged = trimmedCert !== "";
  const autoCreateChanged = autoCreate !== instance.auto_create_wrapper_assets;
  const canSubmit =
    trimmedDisplayName !== "" &&
    (displayNameChanged || storeChanged || certChanged || autoCreateChanged) &&
    !mut.isPending;

  return (
    <Dialog open onOpenChange={(o) => !o && onClose()}>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>Edit: {instance.display_name}</DialogTitle>
          <DialogDescription>
            Rename, remap the target store, or replace the registered
            certificate. Changes take effect on the next runtime request.
          </DialogDescription>
        </DialogHeader>
        <div className="space-y-3">
          <div className="rounded-md border border-(--color-line) bg-(--color-surface-1) px-3 py-2 text-xs text-(--color-muted)">
            Current fingerprint:{" "}
            <code>{instance.current_credential_fingerprint}</code>
          </div>
          <div className="space-y-1.5">
            <Label htmlFor="edit-display-name">Display name</Label>
            <Input
              id="edit-display-name"
              value={displayName}
              onChange={(e) => setDisplayName(e.target.value)}
              placeholder="laptop-work"
            />
          </div>
          <div className="space-y-1.5">
            <Label htmlFor="edit-store">Target store</Label>
            <Select
              id="edit-store"
              value={storeId}
              onChange={(e) => setStoreId(e.target.value)}
            >
              {stores.map((store) => (
                <option key={store.id} value={store.id}>
                  {store.name}
                  {store.id === instance.current_store_id ? " (current)" : ""}
                </option>
              ))}
            </Select>
          </div>
          <div className="space-y-1.5">
            <Label htmlFor="edit-cert-pem">New certificate PEM (optional)</Label>
            <textarea
              id="edit-cert-pem"
              value={certPem}
              onChange={(e) => setCertPem(e.target.value)}
              rows={8}
              spellCheck={false}
              placeholder={"-----BEGIN CERTIFICATE-----\n...\n-----END CERTIFICATE-----"}
              className="flex w-full rounded-md border border-(--color-line) bg-(--color-surface-0) px-3 py-2 font-mono text-xs text-(--color-fg)"
            />
            <p className="text-xs text-(--color-muted)">
              Generate a new credential on the client host with{" "}
              <code>underwrap-keygen</code> and paste the cert PEM to rotate.
              Leave blank to keep the existing cert.
            </p>
          </div>
          <label className="flex items-start gap-2 text-xs text-(--color-muted)">
            <input
              type="checkbox"
              data-testid="edit-auto-create"
              checked={autoCreate}
              onChange={(e) => setAutoCreate(e.target.checked)}
              className="mt-0.5"
            />
            <span>
              <strong className="text-(--color-fg)">
                Auto-create wrapper assets on register
              </strong>
              <br />
              When on, this instance's wrappers may add new resources
              and wrapper/resource pairs on startup.
            </span>
          </label>
          {submitError ? <Alert>{submitError}</Alert> : null}
        </div>
        <DialogFooter>
          <Button type="button" variant="ghost" onClick={onClose}>
            Cancel
          </Button>
          <Button
            type="button"
            disabled={!canSubmit}
            onClick={() =>
              mut.mutate({
                ...(displayNameChanged ? { display_name: trimmedDisplayName } : {}),
                ...(storeChanged ? { current_store_id: storeId } : {}),
                ...(certChanged ? { cert_pem: trimmedCert } : {}),
                ...(autoCreateChanged
                  ? { auto_create_wrapper_assets: autoCreate }
                  : {}),
              })
            }
          >
            {mut.isPending ? "Saving…" : "Save"}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}

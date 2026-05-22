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
  type AdminClientInstanceCreateRequest,
  type AdminStoresListResponse,
  postClientInstanceCreate,
} from "@/api/client";
import { describeApiError } from "@/lib/error-message";

export function CreateClientInstanceDialog({
  stores,
  onClose,
  onSuccess,
}: {
  stores: AdminStoresListResponse["items"];
  onClose: () => void;
  onSuccess: () => Promise<void>;
}) {
  const [displayName, setDisplayName] = useState("");
  const [storeId, setStoreId] = useState(stores[0]?.id ?? "");
  const [status, setStatus] = useState<"active" | "disabled">("active");
  const [certPem, setCertPem] = useState("");
  const [autoCreate, setAutoCreate] = useState(false);
  const [submitError, setSubmitError] = useState<string | null>(null);

  const mut = useMutation({
    mutationFn: (body: AdminClientInstanceCreateRequest) =>
      postClientInstanceCreate(body),
    onSuccess: async () => {
      setSubmitError(null);
      await onSuccess();
    },
    onError: (err) => setSubmitError(describeApiError(err)),
  });

  const trimmedCert = certPem.trim();
  const canSubmit =
    displayName.trim() !== "" &&
    storeId !== "" &&
    trimmedCert !== "" &&
    !mut.isPending;

  return (
    <Dialog open onOpenChange={(o) => !o && onClose()}>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>Create client instance</DialogTitle>
          <DialogDescription>
            Generate a credential on the client host with
            <code className="mx-1">underwrap-keygen</code>
            and paste the certificate PEM below. The private key never leaves the
            client host; only the certificate is registered here.
          </DialogDescription>
        </DialogHeader>
        <div className="space-y-3">
          <div className="space-y-1.5">
            <Label htmlFor="create-display-name">Display name</Label>
            <Input
              id="create-display-name"
              value={displayName}
              onChange={(e) => setDisplayName(e.target.value)}
              placeholder="laptop-work"
            />
          </div>
          <div className="space-y-1.5">
            <Label htmlFor="create-store">Target store</Label>
            <Select
              id="create-store"
              value={storeId}
              onChange={(e) => setStoreId(e.target.value)}
            >
              {stores.map((store) => (
                <option key={store.id} value={store.id}>
                  {store.name}
                </option>
              ))}
            </Select>
          </div>
          <div className="space-y-1.5">
            <Label htmlFor="create-status">Status</Label>
            <Select
              id="create-status"
              value={status}
              onChange={(e) => setStatus(e.target.value as "active" | "disabled")}
            >
              <option value="active">Active</option>
              <option value="disabled">Disabled</option>
            </Select>
          </div>
          <div className="space-y-1.5">
            <Label htmlFor="create-cert-pem">Certificate PEM</Label>
            <textarea
              id="create-cert-pem"
              value={certPem}
              onChange={(e) => setCertPem(e.target.value)}
              rows={8}
              spellCheck={false}
              placeholder={"-----BEGIN CERTIFICATE-----\n...\n-----END CERTIFICATE-----"}
              className="flex w-full rounded-md border border-(--color-line) bg-(--color-surface-0) px-3 py-2 font-mono text-xs text-(--color-fg)"
            />
          </div>
          <label className="flex items-start gap-2 text-xs text-(--color-muted)">
            <input
              type="checkbox"
              data-testid="create-auto-create"
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
              and wrapper/resource pairs on startup. When off, the
              backend refuses to create them and logs which entries it
              expected.
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
                display_name: displayName.trim(),
                current_store_id: storeId,
                status,
                cert_pem: trimmedCert,
                auto_create_wrapper_assets: autoCreate ? true : undefined,
              })
            }
          >
            {mut.isPending ? "Creating…" : "Create"}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
